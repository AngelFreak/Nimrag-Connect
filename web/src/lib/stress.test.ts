import { describe, expect, it } from 'vitest';
import type { StressSample } from '../api/generated/StressSample';
import { daySentence, dominantLevel, elevatedWindow, levelSeconds, levelShares, stressLevelOf } from './stress';

const sample = (time: string, level: StressSample['level']): StressSample => ({ time: `2026-11-18T${time}:00+01:00`, stress: 50, level });
const block = (rest_s: number | null, low_s: number | null, medium_s: number | null, high_s: number | null, series: StressSample[] = []) => ({ rest_s, low_s, medium_s, high_s, activity_s: null, series });

describe('stress levels', () => {
  it('colours by the thresholds 0-25, 26-50, 51-75, 76-100', () => {
    expect([0, 25, 26, 50, 51, 75, 76, 100].map(stressLevelOf)).toEqual(['rest', 'rest', 'low', 'low', 'medium', 'medium', 'high', 'high']);
  });

  it('gives shares that add up to exactly 100 and minutes from the seconds', () => {
    const shares = levelShares(levelSeconds({ rest_s: 3600, low_s: 3600, medium_s: 3600, high_s: null, activity_s: 0 }));
    expect(shares.map((s) => s.level)).toEqual(['rest', 'low', 'medium']);
    expect(shares.reduce((a, s) => a + s.percent, 0)).toBe(100);
    expect(shares.map((s) => s.minutes)).toEqual([60, 60, 60]);
    const odd = levelShares(levelSeconds({ rest_s: 1, low_s: 1, medium_s: 1, high_s: 1, activity_s: 3 }));
    expect(odd.reduce((a, s) => a + s.percent, 0)).toBe(100);
  });

  it('has no shares without time', () => {
    expect(levelShares(levelSeconds(block(null, 0, null, null)))).toEqual([]);
  });

  it('picks the calmer level on a tie', () => {
    expect(dominantLevel(levelSeconds(block(100, 100, 0, 0)))).toBe('rest');
    expect(dominantLevel(levelSeconds(block(null, null, null, null)))).toBeNull();
  });
});

describe('daySentence', () => {
  it('says nothing without readings', () => {
    expect(daySentence(block(null, null, null, null))).toBeNull();
  });

  it('describes a calm day', () => {
    expect(daySentence(block(20000, 3000, 300, 0))).toBe('Mostly resting; little medium or high stress.');
  });

  it('names the medium stretch and when it was', () => {
    const series = [sample('14:10', 'medium'), sample('14:40', 'medium'), sample('15:20', 'medium'), sample('09:00', 'medium'), sample('08:00', 'rest')];
    expect(daySentence(block(20000, 3000, 2400, 0, series))).toBe('Mostly resting; 40 minutes of medium stress, most of it between 14:00 and 16:00.');
  });

  it('names both medium and high, and leaves the time out when it is spread', () => {
    const series = [sample('08:00', 'medium'), sample('12:00', 'high'), sample('17:00', 'medium'), sample('21:00', 'high')];
    expect(daySentence(block(1000, 2000, 4500, 1200, series))).toBe('Mostly medium stress; 20 minutes of high and 1 hour 15 minutes of medium stress.');
  });

  it('finds the window on whole hours', () => {
    expect(elevatedWindow([sample('22:30', 'high'), sample('23:10', 'high')])).toEqual({ from: '22:00', to: '24:00' });
    expect(elevatedWindow([])).toBeNull();
  });
});
