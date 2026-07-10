// @ts-nocheck
/*
 * generate-fixtures.mjs — synthesize the three baseline roast fixtures named in
 * apps/droptime-logger/CONTRACTS.md §3, in the RoastFixture shape (types.ts).
 *
 * Plain Node, zero dependencies. Modeled on the platform's own curve generators:
 *   - apps/droptime-app/convex/seed.ts  (makeRawCurve: linear charge→turning
 *     point, exponential approach to drop, optional post-dry-end stall)
 *   - apps/droptime-web/components/mocks/roastData.ts (numerically-plausible RoR)
 *
 * Run:  node scripts/generate-fixtures.mjs   (writes packages/roast-console/fixtures/*.json)
 *
 * Fixtures store BT/ET only — RoR is display-derived (deriveRoRSeries), never
 * stored raw. The Rust replay driver and the TS SimulatorSource replay these
 * SAME files, so keep them canonical.
 */
import { writeFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURES_DIR = join(__dirname, '..', 'fixtures');

const round1 = (n) => Math.round(n * 10) / 10;

/**
 * Build a BT/ET curve, seconds-from-charge. Linear drop from charge to the
 * turning point, then an exponential approach to the drop temperature. An
 * optional stall flattens BT just after dry-end (RoR crashes) and then recovers
 * smoothly toward the base curve — the anomaly the console's readout should catch.
 */
function makeCurve({
  dropSec,
  tpSec,
  tpTemp,
  chargeTemp,
  dropTemp,
  dryEndSec,
  k = 2.3,
  crash = false,
  stepSec = 5,
}) {
  const base = (t) => {
    if (t <= tpSec) return chargeTemp + (tpTemp - chargeTemp) * (t / tpSec);
    const frac = (t - tpSec) / (dropSec - tpSec);
    return tpTemp + (dropTemp - tpTemp) * ((1 - Math.exp(-k * frac)) / (1 - Math.exp(-k)));
  };

  const stallStart = dryEndSec + 10;
  const stallEnd = dryEndSec + 70;
  const baseAtStall = base(stallStart);
  const crashBtEnd = baseAtStall + 0.02 * (stallEnd - stallStart); // ~1.2°F over 60s
  const offsetEnd = base(stallEnd) - crashBtEnd; // depth below the base curve at recovery

  const btAt = (t) => {
    if (!crash) return base(t);
    if (t < stallStart) return base(t);
    if (t <= stallEnd) return baseAtStall + 0.02 * (t - stallStart); // near-flat → RoR crash
    // Smooth, continuous recovery back onto the base curve (momentum returns).
    return base(t) - offsetEnd * Math.exp(-(t - stallEnd) / 45);
  };

  const curve = [];
  const push = (t) => {
    const bt = btAt(t);
    // ET rides above BT: a wide drum/bean gap at charge that narrows to drop.
    const gap = 35 - 23 * (t / dropSec);
    curve.push({ t, bt: round1(bt), et: round1(bt + gap) });
  };
  for (let t = 0; t <= dropSec; t += stepSec) push(t);
  if (curve[curve.length - 1].t !== dropSec) push(dropSec);
  return curve;
}

/** Quick numerically-integrated RoR check (°F/min) for eyeballing realism. */
function rorAround(curve, tCenter, windowSec = 30) {
  let lo = curve[0];
  let hi = curve[curve.length - 1];
  for (const p of curve) {
    if (p.t <= tCenter - windowSec / 2) lo = p;
    if (p.t <= tCenter + windowSec / 2) hi = p;
  }
  const dt = (hi.t - lo.t) / 60;
  return dt > 0 ? round1((hi.bt - lo.bt) / dt) : 0;
}

const SPECS = [
  {
    name: 'ethiopia-guji',
    description: 'Clean 11-minute washed Ethiopia — even ramp, textbook RoR decline.',
    coffeeName: 'Ethiopia Guji — Hambela',
    chargeWeightLb: 24,
    p: { chargeTemp: 388, tpSec: 82, tpTemp: 173, dryEndSec: 300, fcStartSec: 560, dropSec: 660, dropTemp: 409, k: 2.3 },
  },
  {
    name: 'colombia-stall',
    description: 'Colombia with a post-dry-end stall — RoR crashes then recovers (exercises anomaly UI).',
    coffeeName: 'Colombia Huila — Andrés Ruiz',
    chargeWeightLb: 30,
    p: { chargeTemp: 392, tpSec: 88, tpTemp: 179, dryEndSec: 305, fcStartSec: 590, dropSec: 730, dropTemp: 419, k: 2.3, crash: true },
  },
  {
    name: 'fast-decaf',
    description: 'Fast 9-minute decaf — steep RoR decline, early drop.',
    coffeeName: 'Sumatra Decaf — Ketiara MWP',
    chargeWeightLb: 20,
    p: { chargeTemp: 400, tpSec: 76, tpTemp: 176, dryEndSec: 250, fcStartSec: 450, dropSec: 540, dropTemp: 406, k: 2.9 },
  },
];

mkdirSync(FIXTURES_DIR, { recursive: true });

for (const spec of SPECS) {
  const { p } = spec;
  const curve = makeCurve(p);
  const fcEndSec = Math.min(p.dropSec - 20, p.fcStartSec + 50);
  const fixture = {
    name: spec.name,
    description: spec.description,
    sampleIntervalSec: 5,
    curve,
    markers: {
      chargeTempF: p.chargeTemp,
      turningPointSec: p.tpSec,
      turningPointTempF: p.tpTemp,
      dryEndSec: p.dryEndSec,
      fcStartSec: p.fcStartSec,
      fcEndSec,
      dropSec: p.dropSec,
      dropTempF: p.dropTemp,
      chargeWeightLb: spec.chargeWeightLb,
      coffeeName: spec.coffeeName,
    },
  };
  const file = join(FIXTURES_DIR, `${spec.name}.json`);
  writeFileSync(file, JSON.stringify(fixture, null, 2) + '\n');

  const dryRor = rorAround(curve, p.dryEndSec);
  const stallRor = p.crash ? rorAround(curve, p.dryEndSec + 40) : null;
  const fcRor = rorAround(curve, p.fcStartSec);
  console.log(
    `wrote ${spec.name}.json  points=${curve.length}  ` +
      `charge=${curve[0].bt}°F  tp=${p.tpTemp}°F@${p.tpSec}s  ` +
      `RoR@dry=${dryRor}  ${stallRor !== null ? `RoR@stall=${stallRor}  ` : ''}` +
      `RoR@fc=${fcRor}  drop=${p.dropTemp}°F@${p.dropSec}s`
  );
}

console.log(`\n${SPECS.length} fixtures written to ${FIXTURES_DIR}`);
