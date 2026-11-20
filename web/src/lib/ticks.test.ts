import { describe, expect, it } from 'vitest';
import { extremeTicks, fmtPaceTick, formatTicks, niceTicks, tickCountFor, unitTicks, zeroBasedScale } from './ticks';

describe('niceTicks', () => {
  it('gives round values inside the range', () => {
    expect(niceTicks(0, 100, 5)).toEqual([0, 25, 50, 75, 100]);
    expect(niceTicks(0, 100, 3)).toEqual([0, 50, 100]);
    expect(niceTicks(48, 130, 4)).toEqual([60, 80, 100, 120]);
  });
  it('never leaves the range', () => {
    for (const [lo, hi] of [
      [47.3, 131.9],
      [0.2, 0.9],
      [5400, 11800],
      [-12, 40],
    ] as const) {
      const t = niceTicks(lo, hi, 4);
      expect(t.length).toBeGreaterThanOrEqual(2);
      expect(Math.min(...t)).toBeGreaterThanOrEqual(lo);
      expect(Math.max(...t)).toBeLessThanOrEqual(hi);
    }
  });
  it('keeps to the requested count (3 to 5 for the usual 4)', () => {
    for (const [lo, hi] of [
      [48, 148],
      [0, 10000],
      [55, 71],
      [61.4, 64.9],
      [68.2, 71.7],
    ] as const) {
      const n = niceTicks(lo, hi, 4).length;
      expect(n).toBeGreaterThanOrEqual(3);
      expect(n).toBeLessThanOrEqual(5);
    }
  });
  it('whole numbers only for integer axes', () => {
    expect(niceTicks(61.2, 63.9, 4, { integer: true })).toEqual([62, 63]);
    expect(unitTicks(14.1, 16.3, 4, 'int').every(Number.isInteger)).toBe(true);
  });
  it('handles a flat range and bad input', () => {
    expect(niceTicks(5, 5)).toEqual([5]);
    expect(niceTicks(Number.NaN, 5)).toEqual([]);
    expect(niceTicks(100, 0, 3)).toEqual([0, 50, 100]);
  });
  it('has no floating point noise', () => {
    expect(niceTicks(0, 1, 5)).toEqual([0, 0.25, 0.5, 0.75, 1]);
    expect(niceTicks(0.1, 0.5, 5)).toEqual([0.1, 0.2, 0.3, 0.4, 0.5]);
  });
});

describe('unit ticks and labels', () => {
  it('writes integers with thousands separators', () => {
    expect(formatTicks([0, 5000, 10000])).toEqual(['0', '5,000', '10,000']);
  });
  it('writes pace as m:ss on quarter-minute steps', () => {
    const t = unitTicks(5.1, 6.4, 4, 'pace');
    expect(t).toEqual([5.25, 5.5, 5.75, 6, 6.25]);
    expect(formatTicks(t, 'pace')).toEqual(['5:15', '5:30', '5:45', '6:00', '6:15']);
    expect(fmtPaceTick(5.5)).toBe('5:30');
  });
  it('writes durations as hours', () => {
    expect(formatTicks(unitTicks(5.2, 8.6, 4, 'hours'), 'hours')).toEqual(['6h', '7h', '8h']);
    expect(formatTicks([360, 420, 480], 'minutes')).toEqual(['6h', '7h', '8h']);
    expect(formatTicks([330, 360, 390], 'minutes')).toEqual(['5.5h', '6.0h', '6.5h']);
  });
  it('shows one decimal only when the ticks need it', () => {
    expect(formatTicks([70, 75, 80], 'decimal')).toEqual(['70', '75', '80']);
    expect(formatTicks([70.5, 71, 71.5], 'decimal')).toEqual(['70.5', '71.0', '71.5']);
  });
});

describe('scale helpers', () => {
  it('fits tick labels to the height and font size', () => {
    expect(tickCountFor(48, 12)).toBe(2);
    expect(tickCountFor(90, 15.6)).toBe(3);
    expect(tickCountFor(120, 12)).toBe(5);
    expect(tickCountFor(48, 15.6)).toBe(2);
    expect(tickCountFor(300, 12)).toBe(5);
    expect(tickCountFor(10, 12)).toBe(2);
  });
  it('sparklines get the extremes only', () => {
    expect(extremeTicks([null, 14, 16, 15, null])).toEqual([14, 16]);
    expect(extremeTicks([7, 7])).toEqual([7]);
    expect(extremeTicks([null])).toEqual([]);
  });
  it('rounds a bar scale up to a tick', () => {
    const s = zeroBasedScale(9120, 4, 'int');
    expect(s.top).toBeGreaterThanOrEqual(9120);
    expect(s.ticks[s.ticks.length - 1]).toBe(s.top);
    expect(zeroBasedScale(10000, 3, 'int')).toEqual({ ticks: [0, 5000, 10000], top: 10000 });
  });
});
