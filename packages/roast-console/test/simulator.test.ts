import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { SimulatorSource, BUNDLED_FIXTURES } from '../src/simulator';
import type { LiveSample, RoastFixture, SourceStatus, SimulatorOptions } from '../src/types';

const TINY: RoastFixture = {
  name: 'tiny',
  description: 'unit-test fixture',
  sampleIntervalSec: 5,
  curve: [
    { t: 0, bt: 400, et: 420 },
    { t: 10, bt: 380, et: 400 },
    { t: 20, bt: 390, et: 405 },
  ],
  markers: { chargeTempF: 400, dropSec: 20, dropTempF: 390 },
};

/** Drive a source to completion under fake timers and collect what it emitted. */
function drive(fixture: RoastFixture, opts?: SimulatorOptions) {
  const samples: LiveSample[] = [];
  const statuses: SourceStatus[] = [];
  const src = new SimulatorSource(fixture, opts);
  void src.start({ onSample: (s) => samples.push(s), onStatus: (st) => statuses.push(st) });
  vi.advanceTimersByTime(60 * 60 * 1000); // an hour of fake time — well past any fixture
  return { samples, statuses, src };
}

describe('SimulatorSource', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('is deterministic with no noise or dropout', () => {
    const a = drive(TINY);
    const b = drive(TINY);
    expect(a.samples).toEqual(b.samples);
  });

  it('is reproducible even with seeded noise + dropout', () => {
    const opts: SimulatorOptions = { noiseF: 2, dropoutP: 0.25 };
    const a = drive(BUNDLED_FIXTURES['ethiopia-guji']!, opts);
    const b = drive(BUNDLED_FIXTURES['ethiopia-guji']!, opts);
    expect(a.samples).toEqual(b.samples);
  });

  it('interpolates the coarse fixture up to a 1s emit interval', () => {
    const { samples } = drive(TINY, { sampleIntervalSec: 1 });
    // t = 0..20 inclusive at 1s → 21 samples
    expect(samples).toHaveLength(21);
    expect(samples.map((s) => s.sessionSec)).toEqual(Array.from({ length: 21 }, (_, i) => i));
    // BT interpolated at t=5 is halfway between 400 and 380 → 390
    expect(samples.find((s) => s.sessionSec === 5)!.btF).toBeCloseTo(390, 5);
  });

  it('emits a connected status first and an ended status when the fixture is exhausted', () => {
    const { statuses } = drive(TINY);
    expect(statuses[0]!.kind).toBe('connected');
    expect(statuses[statuses.length - 1]!.kind).toBe('ended');
    expect(statuses.filter((s) => s.kind === 'ended')).toHaveLength(1);
  });

  it('produces monotone, gapless seq at the fixture cadence when dropoutP is 0', () => {
    const { samples } = drive(TINY);
    expect(samples.map((s) => s.seq)).toEqual([0, 1, 2, 3, 4]); // t = 0,5,10,15,20
    expect(samples.map((s) => s.sessionSec)).toEqual([0, 5, 10, 15, 20]);
  });

  it('drops samples (seq gaps) when dropoutP > 0', () => {
    const fx = BUNDLED_FIXTURES['ethiopia-guji']!; // 133 ticks at the 5s cadence
    const totalTicks = fx.curve[fx.curve.length - 1]!.t / fx.sampleIntervalSec + 1;
    const { samples } = drive(fx, { dropoutP: 0.5 });
    expect(samples.length).toBeLessThan(totalTicks);
    expect(samples.length).toBeGreaterThan(0);
    // seq is the tick index, so a dropped sample shows up as a jump > 1.
    const seqs = samples.map((s) => s.seq);
    const hasGap = seqs.some((s, i) => i > 0 && s - seqs[i - 1]! > 1);
    expect(hasGap).toBe(true);
    // every emitted seq is still within range and strictly increasing
    expect(seqs.every((s, i) => i === 0 || s > seqs[i - 1]!)).toBe(true);
  });

  it('synthesizes ~60s of preheat drift when startAtCharge is false', () => {
    const { samples } = drive(TINY, { startAtCharge: false });
    const first = samples[0]!;
    expect(first.sessionSec).toBe(0);
    // preheat begins ~8°F above the charge temp and drifts down into charge
    expect(first.btF).toBeCloseTo(408, 1);
    // charge (roast t=0) lands at sessionSec = 60 with the fixture's charge temp
    const charge = samples.find((s) => s.sessionSec === 60)!;
    expect(charge.btF).toBeCloseTo(400, 5);
    // and there is genuinely negative-t (pre-charge) data, i.e. samples before 60s
    expect(samples.some((s) => s.sessionSec < 60)).toBe(true);
  });

  it('respects the speed multiplier in wall-clock scheduling', () => {
    const samples: LiveSample[] = [];
    const src = new SimulatorSource(TINY, { speed: 10 });
    void src.start({ onSample: (s) => samples.push(s), onStatus: () => {} });
    // fixture spans 20 roast-seconds at 5s cadence; at 10x, samples are 500ms apart.
    vi.advanceTimersByTime(400); // not yet time for the 2nd sample (t=0 fired at 0ms)
    expect(samples).toHaveLength(1);
    vi.advanceTimersByTime(200); // now past 500ms → 2nd sample
    expect(samples).toHaveLength(2);
    void src.stop();
  });

  it('stop() halts emission', () => {
    const samples: LiveSample[] = [];
    const src = new SimulatorSource(BUNDLED_FIXTURES['colombia-stall']!, { speed: 1 });
    void src.start({ onSample: (s) => samples.push(s), onStatus: () => {} });
    vi.advanceTimersByTime(5000); // ~1 sample at 5s cadence, speed 1
    const countAtStop = samples.length;
    void src.stop();
    vi.advanceTimersByTime(60 * 60 * 1000);
    expect(samples.length).toBe(countAtStop);
  });

  it('exposes a simulator SourceInfo derived from the fixture', () => {
    const src = new SimulatorSource(TINY);
    expect(src.info).toEqual({ id: 'simulator:tiny', label: 'unit-test fixture', kind: 'simulator' });
  });
});

describe('BUNDLED_FIXTURES', () => {
  it('ships the three baseline fixtures from CONTRACTS §3', () => {
    expect(Object.keys(BUNDLED_FIXTURES).sort()).toEqual(['colombia-stall', 'ethiopia-guji', 'fast-decaf']);
  });

  it('each fixture is well-formed and seconds-from-charge', () => {
    for (const fx of Object.values(BUNDLED_FIXTURES)) {
      expect(fx.curve.length).toBeGreaterThan(20);
      expect(fx.curve[0]!.t).toBe(0);
      expect(fx.sampleIntervalSec).toBeGreaterThan(0);
      // monotone non-decreasing time base
      for (let i = 1; i < fx.curve.length; i++) {
        expect(fx.curve[i]!.t).toBeGreaterThan(fx.curve[i - 1]!.t);
      }
      // realistic charge/drop temperatures
      expect(fx.markers.chargeTempF!).toBeGreaterThanOrEqual(380);
      expect(fx.markers.chargeTempF!).toBeLessThanOrEqual(430);
      expect(fx.markers.dropTempF!).toBeGreaterThan(395);
      expect(fx.markers.dropTempF!).toBeLessThan(425);
    }
  });
});
