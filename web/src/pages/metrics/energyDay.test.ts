import { describe, expect, it } from 'vitest';
import type { MetricRange } from '../../api/generated/MetricRange';
import { daySentence, energyPoints, fallsIn, flowChart, risesIn, type DayFacts, type EnergyPoint } from './energyDay';

const H = 3_600_000;
/** Samples at whole hours from 0. */
const pts = (values: number[]): EnergyPoint[] => values.map((v, i) => ({ t: i * H, v }));

describe('energy sums', () => {
  it('sorts the series and drops samples without a value or a time', () => {
    expect(
      energyPoints([
        { time: '2026-11-19T02:00:00+00:00', value: 50 },
        { time: '2026-11-19T01:00:00+00:00', value: null },
        { time: 'broken', value: 40 },
        { time: '2026-11-19T00:00:00+00:00', value: 30 },
      ]).map((p) => p.v),
    ).toEqual([30, 50]);
  });

  it('adds rises and falls between neighbours, each to the window of its later sample', () => {
    const p = pts([20, 50, 40, 90, 60]);
    // Rises 30 and 50; falls 10 and 30.
    expect(risesIn(p, -Infinity, Infinity)).toBe(80);
    expect(fallsIn(p, -Infinity, Infinity)).toBe(40);
    // From hour 1 (excluded) to hour 3 (included): the fall to 40 and the rise to 90.
    expect(risesIn(p, 1 * H, 3 * H)).toBe(50);
    expect(fallsIn(p, 1 * H, 3 * H)).toBe(10);
    expect(risesIn(p, 4 * H, 9 * H)).toBe(0);
  });
});

const facts = (over: Partial<DayFacts> = {}): DayFacts => ({
  points: pts([20, 60, 90, 80, 50, 40]),
  sleep: null,
  activities: [],
  stressAverage: 30,
  isToday: false,
  now: null,
  ...over,
});

describe('the sentence under the timeline', () => {
  it('has none without enough samples', () => {
    expect(daySentence(facts({ points: pts([50]) }))).toBeNull();
    expect(daySentence(facts({ points: [] }))).toBeNull();
  });

  it('says how much sleep recharged, from the rises while asleep', () => {
    // Asleep from hour 0 to 2: the rises 40 and 30.
    const s = daySentence(facts({ sleep: { start: 0, end: 2 * H } }));
    expect(s).toContain('Sleep recharged you by 70.');
    expect(daySentence(facts({ sleep: { start: 3 * H, end: 4 * H } }))).toContain('Sleep added little energy.');
    // No night in the day: no sleep sentence.
    expect(daySentence(facts())).not.toMatch(/Sleep/);
  });

  it('names the activities when they took at least half of the drain', () => {
    // Falls: 10, 30, 10 = 50. A run from hour 3 to 4 covers the 30 (arriving at hour 4).
    const s = daySentence(facts({ activities: [{ name: 'Run', start: 3 * H, end: 4 * H }] }));
    expect(s).toContain('Your run drained most, −30 of −50.');
    const two = daySentence(facts({ activities: [{ name: 'Run', start: 3 * H, end: 4 * H }, { name: 'Strength', start: 4 * H, end: 5 * H }] }));
    expect(two).toContain('Your activities drained most, −40 of −50.');
  });

  it('otherwise blames stress when it was noticeable, and everyday movement when it was not', () => {
    const calm = daySentence(facts({ activities: [{ name: 'Walk', start: 2 * H, end: 2 * H + 1000 }], stressAverage: 30 }));
    expect(calm).toContain('Everyday movement drained most, −50 of −50.');
    const tense = daySentence(facts({ stressAverage: 40 }));
    expect(tense).toContain('Stress and daily movement drained most, −50 of −50.');
    expect(daySentence(facts({ stressAverage: null }))).toContain('Everyday movement');
  });

  it('says so when almost nothing drained the energy', () => {
    expect(daySentence(facts({ points: pts([20, 40, 60, 80]) }))).toContain('Little drained your energy.');
  });

  it('ends with where the day stands: now for today, the last value for a finished day', () => {
    expect(daySentence(facts({ isToday: true, now: 42 }))).toMatch(/It is at 42 now\.$/);
    expect(daySentence(facts({ isToday: false }))).toMatch(/The day ended at 40\.$/);
  });
});

describe('charged and drained per day', () => {
  const range = (over: Partial<MetricRange> = {}): MetricRange => ({
    sample: false,
    metric: 'energy',
    range: '7d',
    from: '2026-11-13',
    to: '2026-11-19',
    bucket: 'day',
    series: [
      { key: 'charged', label: 'Charged', unit: '', points: ['13', '14', '15', '16', '17', '18', '19'].map((d, i) => ({ date: `2026-11-${d}`, value: i === 1 ? null : 60 + i })) },
      { key: 'drained', label: 'Drained', unit: '', points: ['13', '14', '15', '16', '17', '18', '19'].map((d, i) => ({ date: `2026-11-${d}`, value: i === 1 ? null : 50 + i })) },
    ],
    stats: [],
    goal: null,
    target_low: null,
    target_high: null,
    has_prev: true,
    has_next: false,
    ...over,
  });

  it('keeps the values, labels a week by weekday and reads out the numbers', () => {
    const c = flowChart(range(), (d) => d.slice(8));
    expect(c.hasData).toBe(true);
    expect(c.charged).toEqual([60, null, 62, 63, 64, 65, 66]);
    expect(c.labels).toEqual(['13', '14', '15', '16', '17', '18', '19']);
    expect(c.summary).toContain('13 +60 charged, 50 drained');
    expect(c.summary).not.toContain('14 +');
  });

  it('says there are no readings when every day is empty', () => {
    const empty = range({ series: range().series.map((s) => ({ ...s, points: s.points.map((p) => ({ ...p, value: null })) })) });
    const c = flowChart(empty, (d) => d);
    expect(c.hasData).toBe(false);
    expect(c.summary).toBe('No energy readings in this period.');
  });
});
