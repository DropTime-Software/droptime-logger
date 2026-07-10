import { describe, it, expect } from 'vitest';
import {
  fmtTemp,
  fmtTempDelta,
  fromDisplayTemp,
  mmss,
  splitFixed,
  tempSuffix,
  toDisplayTemp,
} from '../src/chart/format';

// The display formatters convert °F → the user's unit for TEXT ONLY — chart
// geometry and stored data always stay canonical °F. These guard the °C path.

describe('toDisplayTemp / fromDisplayTemp', () => {
  it('leaves °F untouched', () => {
    expect(toDisplayTemp(400, 'F')).toBe(400);
    expect(fromDisplayTemp(400, 'F')).toBe(400);
  });

  it('converts °F → °C on known anchors', () => {
    expect(toDisplayTemp(32, 'C')).toBeCloseTo(0, 10);
    expect(toDisplayTemp(212, 'C')).toBeCloseTo(100, 10);
    expect(toDisplayTemp(400, 'C')).toBeCloseTo(204.444, 3);
  });

  it('round-trips display → °F → display for both units', () => {
    for (const unit of ['F', 'C'] as const) {
      for (const f of [32, 100, 212, 356.7, 401.2]) {
        expect(fromDisplayTemp(toDisplayTemp(f, unit), unit)).toBeCloseTo(f, 9);
      }
    }
  });
});

describe('fmtTemp / tempSuffix', () => {
  it('formats with the requested precision in each unit', () => {
    expect(fmtTemp(401.4, 'F')).toBe('401');
    expect(fmtTemp(401.4, 'F', 1)).toBe('401.4');
    expect(fmtTemp(212, 'C', 1)).toBe('100.0');
  });

  it('labels the unit', () => {
    expect(tempSuffix('F')).toBe('°F');
    expect(tempSuffix('C')).toBe('°C');
  });
});

describe('fmtTempDelta', () => {
  it('carries an explicit sign and converts the magnitude', () => {
    expect(fmtTempDelta(5, 'F')).toBe('+5');
    expect(fmtTempDelta(-5, 'F')).toBe('−5');
    // a 9 °F delta is a 5 °C delta (scale only, no +32 offset)
    expect(fmtTempDelta(9, 'C')).toBe('+5');
    expect(fmtTempDelta(-9, 'C')).toBe('−5');
  });
});

describe('splitFixed', () => {
  it('splits whole and fractional parts', () => {
    expect(splitFixed(401.42, 1)).toEqual({ whole: '401', frac: '4' });
    expect(splitFixed(401, 0)).toEqual({ whole: '401', frac: '' });
  });
});

describe('mmss', () => {
  it('formats seconds as m:ss and signs negative (preheat) times', () => {
    expect(mmss(0)).toBe('0:00');
    expect(mmss(65)).toBe('1:05');
    expect(mmss(600)).toBe('10:00');
    expect(mmss(-30)).toBe('−0:30');
    expect(mmss(NaN)).toBe('–:––');
  });
});
