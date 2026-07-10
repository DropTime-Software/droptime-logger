/**
 * INTERNAL display formatting for the chart + console components.
 *
 * These are deliberately NOT re-exported from `chart/index.ts` or the package
 * barrel: the public math surface (`mmss`, `formatTemp`, `fToC`) lives in
 * `../math` per the contract, and duplicating the names in another star-export
 * would make the barrel ambiguous. Keep these private and tiny.
 */

export type TempUnit = 'F' | 'C';

/** m:ss (rounds to whole seconds; negative values — preheat — get a minus sign). */
export function mmss(sec: number): string {
  if (!Number.isFinite(sec)) return '–:––';
  const s = Math.round(Math.abs(sec));
  const sign = sec < 0 && s > 0 ? '−' : '';
  return `${sign}${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/** Canonical °F → display unit. Geometry always stays in °F; this is display-only. */
export function toDisplayTemp(f: number, unit: TempUnit): number {
  return unit === 'C' ? ((f - 32) * 5) / 9 : f;
}

/** Inverse of `toDisplayTemp`: a value typed in the display unit → canonical °F. */
export function fromDisplayTemp(v: number, unit: TempUnit): number {
  return unit === 'C' ? (v * 9) / 5 + 32 : v;
}

export function fmtTemp(f: number, unit: TempUnit, digits = 0): string {
  return toDisplayTemp(f, unit).toFixed(digits);
}

export function tempSuffix(unit: TempUnit): string {
  return unit === 'C' ? '°C' : '°F';
}

/** A temperature DELTA in °F, formatted in the display unit with an explicit sign. */
export function fmtTempDelta(fDelta: number, unit: TempUnit, digits = 0): string {
  const v = unit === 'C' ? (fDelta * 5) / 9 : fDelta;
  const s = Math.abs(v).toFixed(digits);
  return v < 0 ? `−${s}` : `+${s}`;
}

/** Split a fixed-decimal value into whole/fraction parts for big-number layouts. */
export function splitFixed(v: number, digits = 1): { whole: string; frac: string } {
  const s = v.toFixed(digits);
  const i = s.indexOf('.');
  return i === -1 ? { whole: s, frac: '' } : { whole: s.slice(0, i), frac: s.slice(i + 1) };
}
