/**
 * SimulatorSource — a SampleSource that replays a RoastFixture at real cadence
 * (÷ speed), interpolating the fixture's coarse curve up to the emit interval.
 * The console cannot tell it apart from the Tauri device/replay bridge.
 *
 * - emits at `opts.sampleIntervalSec ?? fixture.sampleIntervalSec` (roast-time),
 *   scheduled with drift correction against an absolute wall clock;
 * - `startAtCharge` (default true) begins at t=0; when false it synthesizes ~60s
 *   of plausible preheat drift before charge (negative t);
 * - gaussian `noiseF` on bt/et; `dropoutP` skips a seq (a detectable gap);
 * - deterministic when noiseF and dropoutP are both 0 (and, thanks to a seeded
 *   PRNG, reproducible even when they are not);
 * - status events per types.ts; 'ended' when the fixture is exhausted (unless
 *   `loop`).
 */
import type {
  LiveSample,
  RoastFixture,
  SampleSource,
  SimulatorOptions,
  SourceInfo,
  SourceListener,
  SourceStatus,
  SourceStatusKind,
} from '../types';

import ethiopiaGuji from '../../fixtures/ethiopia-guji.json';
import colombiaStall from '../../fixtures/colombia-stall.json';
import fastDecaf from '../../fixtures/fast-decaf.json';

const PREHEAT_SEC = 60;
const PREHEAT_DRIFT_F = 8; // gentle probe drift across the synthesized preheat
const RNG_SEED = 0x9e3779b9;

const round1 = (n: number): number => Math.round(n * 10) / 10;
const round3 = (n: number): number => Math.round(n * 1000) / 1000;

/** Deterministic PRNG (mulberry32) so runs are reproducible. */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Standard-normal samples via Box–Muller, drawing from the given uniform PRNG. */
function makeGaussian(rng: () => number): () => number {
  let spare: number | undefined;
  return () => {
    if (spare !== undefined) {
      const s = spare;
      spare = undefined;
      return s;
    }
    let u = 0;
    let v = 0;
    while (u === 0) u = rng();
    while (v === 0) v = rng();
    const mag = Math.sqrt(-2 * Math.log(u));
    spare = mag * Math.sin(2 * Math.PI * v);
    return mag * Math.cos(2 * Math.PI * v);
  };
}

type FixturePoint = { t: number; bt: number; et?: number };

/** Linear interpolation of a fixture field at roast-time t (t within curve span). */
function interp(curve: FixturePoint[], t: number, field: 'bt' | 'et'): number {
  const first = curve[0]!;
  const value = (p: FixturePoint): number => (field === 'bt' ? p.bt : p.et ?? p.bt);
  if (t <= first.t) return value(first);
  for (let i = 0; i < curve.length - 1; i++) {
    const a = curve[i]!;
    const b = curve[i + 1]!;
    if (t >= a.t && t <= b.t) {
      const span = b.t - a.t;
      if (span <= 0) return value(a);
      return value(a) + (value(b) - value(a)) * ((t - a.t) / span);
    }
  }
  return value(curve[curve.length - 1]!);
}

export class SimulatorSource implements SampleSource {
  readonly info: SourceInfo;

  private readonly fixture: RoastFixture;
  private readonly curve: FixturePoint[];
  private readonly hasEt: boolean;
  private readonly speed: number;
  private readonly noiseF: number;
  private readonly dropoutP: number;
  private readonly emitInterval: number;
  private readonly loop: boolean;
  private readonly tStart: number;
  private readonly tEnd: number;
  private readonly fixtureMinT: number;
  private readonly fixtureMaxT: number;
  /** sessionSec at which the replay crosses fixture t=0 (charge): 0 when
   *  startAtCharge, the synthesized-preheat length otherwise. Consumers use
   *  this to auto-mark CHARGE for replays — the fixture already knows it. */
  readonly chargeAtSessionSec: number;

  private listener: SourceListener | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private startWall = 0;
  private g = 0; // scheduling tick, monotonic across loops (keeps cadence smooth)
  private k = 0; // fixture-position tick, resets each loop
  private stopped = false;
  private rng: () => number = mulberry32(RNG_SEED);
  private gaussian: () => number = makeGaussian(this.rng);

  constructor(fixture: RoastFixture, opts: SimulatorOptions = {}) {
    this.fixture = fixture;
    this.curve = fixture.curve as FixturePoint[];
    this.hasEt = this.curve.some((p) => p.et !== undefined);
    this.speed = Math.min(20, Math.max(1, opts.speed ?? 1));
    this.noiseF = Math.max(0, opts.noiseF ?? 0);
    this.dropoutP = Math.min(1, Math.max(0, opts.dropoutP ?? 0));
    this.emitInterval = opts.sampleIntervalSec ?? fixture.sampleIntervalSec ?? 1;
    const startAtCharge = opts.startAtCharge ?? true;
    this.loop = opts.loop ?? false;

    this.fixtureMinT = this.curve.length ? this.curve[0]!.t : 0;
    this.fixtureMaxT = this.curve.length ? this.curve[this.curve.length - 1]!.t : 0;
    this.tStart = startAtCharge
      ? Math.max(0, this.fixtureMinT)
      : Math.min(this.fixtureMinT, -PREHEAT_SEC);
    this.tEnd = this.fixtureMaxT;
    this.chargeAtSessionSec = Math.max(0, -this.tStart);

    this.info = {
      id: `simulator:${fixture.name}`,
      label: fixture.description ?? fixture.name,
      kind: 'simulator',
    };
  }

  start(listener: SourceListener): Promise<void> {
    this.listener = listener;
    this.stopped = false;
    this.g = 0;
    this.k = 0;
    this.rng = mulberry32(RNG_SEED);
    this.gaussian = makeGaussian(this.rng);
    this.startWall = Date.now();
    this.emitStatus('connected', 0);
    this.scheduleNext();
    return Promise.resolve();
  }

  stop(): Promise<void> {
    this.stopped = true;
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    return Promise.resolve();
  }

  private scheduleNext(): void {
    if (this.stopped) return;
    const targetWall = this.startWall + ((this.g * this.emitInterval) / this.speed) * 1000;
    const delay = Math.max(0, targetWall - Date.now());
    this.timer = setTimeout(() => this.tick(), delay);
  }

  private tick(): void {
    if (this.stopped) return;
    const roastT = this.tStart + this.k * this.emitInterval;

    if (roastT > this.tEnd + 1e-9) {
      if (this.loop) {
        this.k = 0;
        this.g += 1;
        this.scheduleNext();
        return;
      }
      this.emitStatus('ended', this.k * this.emitInterval);
      this.timer = null;
      return;
    }

    const sessionSec = round3(roastT - this.tStart); // = k * emitInterval
    const dropped = this.dropoutP > 0 && this.rng() < this.dropoutP;
    if (!dropped) {
      const bt = this.btAt(roastT) + this.noise();
      const sample: LiveSample = { seq: this.k, sessionSec, btF: round1(bt) };
      const et = this.etAt(roastT);
      if (et !== undefined) sample.etF = round1(et + this.noise());
      this.listener?.onSample(sample);
    }

    this.k += 1;
    this.g += 1;
    this.scheduleNext();
  }

  private noise(): number {
    return this.noiseF > 0 ? this.gaussian() * this.noiseF : 0;
  }

  private btAt(t: number): number {
    if (this.curve.length === 0) return 0;
    const anchor = this.curve[0]!;
    if (t < anchor.t) {
      // Synthesized preheat: probe drifts gently down toward the charge temp.
      return anchor.bt + PREHEAT_DRIFT_F * ((anchor.t - t) / PREHEAT_SEC);
    }
    if (t >= this.fixtureMaxT) return this.curve[this.curve.length - 1]!.bt;
    return interp(this.curve, t, 'bt');
  }

  private etAt(t: number): number | undefined {
    if (!this.hasEt || this.curve.length === 0) return undefined;
    const anchor = this.curve[0]!;
    const anchorEt = anchor.et ?? anchor.bt + 20;
    if (t < anchor.t) {
      return anchorEt + PREHEAT_DRIFT_F * ((anchor.t - t) / PREHEAT_SEC);
    }
    if (t >= this.fixtureMaxT) return this.curve[this.curve.length - 1]!.et;
    return interp(this.curve, t, 'et');
  }

  private emitStatus(kind: SourceStatusKind, atSessionSec: number, message?: string): void {
    const status: SourceStatus = { kind, atSessionSec: round3(atSessionSec) };
    if (message !== undefined) status.message = message;
    this.listener?.onStatus(status);
  }
}

const asFixture = (json: unknown): RoastFixture => json as RoastFixture;

/** The bundled baseline fixtures (CONTRACTS.md §3), keyed by fixture name. */
export const BUNDLED_FIXTURES: Record<string, RoastFixture> = {
  'ethiopia-guji': asFixture(ethiopiaGuji),
  'colombia-stall': asFixture(colombiaStall),
  'fast-decaf': asFixture(fastDecaf),
};
