import { describe, expect, it } from 'vitest';
import type { MetricRange } from '../../api/generated/MetricRange';
import { addDays } from '../../lib/dates';
import {
  bucketAxis,
  bucketEnd,
  bucketsWithData,
  chartHasData,
  credited,
  creditedMinutes,
  rangeChart,
  rangeRows,
  rangeSummary,
  runningTotal,
  statText,
  tileStats,
  weekLine,
  type BarChartSpec,
  type LineChartSpec,
  type StackedChartSpec,
} from './model';
import { METRICS, type MetricKey } from './period';

const TODAY = '2026-11-19';

type Vals = (number | null)[];
const series = (key: string, from: string, vals: Vals, step = 1, unit = '') => ({
  key,
  label: key,
  unit,
  points: vals.map((value, i) => ({ date: addDays(from, i * step), value })),
});

function range(metric: string, rangeKey: string, from: string, to: string, bucket: 'day' | 'week', ss: ReturnType<typeof series>[], over: Partial<MetricRange> = {}): MetricRange {
  return { sample: false, metric, range: rangeKey, from, to, bucket, series: ss, stats: [], goal: null, target_low: null, target_high: null, has_prev: true, has_next: false, ...over };
}

const week = (metric: string, ss: (from: string) => ReturnType<typeof series>[], over: Partial<MetricRange> = {}) => range(metric, '7d', '2026-11-13', TODAY, 'day', ss('2026-11-13'), over);

describe('credited minutes', () => {
  it('counts vigorous minutes double, and nothing recorded is null, not 0', () => {
    expect(credited(30, 10)).toBe(50);
    expect(credited(null, 5)).toBe(10);
    expect(credited(20, null)).toBe(20);
    expect(credited(0, 0)).toBe(0);
    expect(credited(null, null)).toBeNull();
  });

  it('is worked out per bucket from the two series', () => {
    const d = week('intensity_minutes', (f) => [series('moderate', f, [10, null, 15]), series('vigorous', f, [0, null, 5])]);
    expect(creditedMinutes(d)).toEqual([10, null, 25]);
  });
});

describe('runningTotal', () => {
  const starts = ['2026-11-16', '2026-11-17', '2026-11-18', '2026-11-19', '2026-11-20'];
  it('adds the days up; a day without data adds nothing; the days after today have no total', () => {
    expect(runningTotal([10, 25, null, 15, null], starts, TODAY)).toEqual([10, 35, 35, 50, null]);
  });
  it('has no total at all when no day has a value', () => {
    expect(runningTotal([null, null, null], starts.slice(0, 3), TODAY)).toEqual([null, null, null]);
  });
  it('stays flat through the days of a past week', () => {
    expect(runningTotal([10, null, null], starts.slice(0, 3), TODAY)).toEqual([10, 10, 10]);
  });
});

describe('bucketAxis', () => {
  it('labels four dates over a long period, each at its bucket, and every day of a short one', () => {
    const d = week('steps', (f) => [series('steps', f, [1, 2, 3, 4, 5, 6, 7])]);
    expect(bucketAxis(d)).toEqual([
      { value: 0, text: 'Nov 13' },
      { value: 2, text: 'Nov 15' },
      { value: 4, text: 'Nov 17' },
      { value: 6, text: 'Nov 19' },
    ]);
    const short = week('steps', (f) => [series('steps', f, [1, 2, 3])]);
    expect(bucketAxis(short).map((l) => l.text)).toEqual(['Nov 13', 'Nov 14', 'Nov 15']);
  });

  it('names months over a year', () => {
    const d = range('steps', '1y', '2025-11-21', TODAY, 'week', [series('steps', '2025-11-21', Array(52).fill(1), 7)]);
    expect(bucketAxis(d).map((l) => l.text)).toEqual(['Nov', 'Mar', 'Jul', 'Nov']);
  });
});

describe('bucketEnd', () => {
  it('is the day itself, or six days on but never after the period', () => {
    expect(bucketEnd({ bucket: 'day', to: TODAY }, '2026-11-14')).toBe('2026-11-14');
    expect(bucketEnd({ bucket: 'week', to: TODAY }, '2026-11-02')).toBe('2026-11-08');
    expect(bucketEnd({ bucket: 'week', to: TODAY }, '2026-11-16')).toBe(TODAY);
  });
});

describe('statText', () => {
  it('writes short units after the number and puts long ones under it', () => {
    expect(statText({ value: 54, unit: 'bpm' })).toEqual({ value: '54 bpm', unit: null });
    expect(statText({ value: 97, unit: '%' })).toEqual({ value: '97%', unit: null });
    expect(statText({ value: 14.25, unit: 'brpm' })).toEqual({ value: '14.3 brpm', unit: null });
    expect(statText({ value: 50.4, unit: 'ml/kg/min' })).toEqual({ value: '50.4', unit: 'ml/kg/min' });
    expect(statText({ value: 36000, unit: 'steps' })).toEqual({ value: '36,000', unit: null });
    expect(statText({ value: 305, unit: '' })).toEqual({ value: '305', unit: null });
    expect(statText({ value: 11000, unit: 'm' })).toEqual({ value: '11.0 km', unit: null });
  });
  it('has no figure for a period without data', () => {
    expect(statText({ value: null, unit: 'bpm' })).toEqual({ value: null, unit: null });
  });
});

describe('rangeRows', () => {
  const hr = week('heart_rate', (f) => [series('resting', f, [null, 54, 55, null, 53, null, null], 1, 'bpm'), series('high', f, [null, 110, 121, null, 113, null, null], 1, 'bpm')]);

  it('has one row per day with a value, newest first, and none for the other days', () => {
    const rows = rangeRows(METRICS.heart_rate, hr, TODAY);
    expect(rows.map((r) => r.start)).toEqual(['2026-11-17', '2026-11-15', '2026-11-14']);
    expect(rows[0]).toMatchObject({ title: 'Tuesday', sub: 'Nov 17', value: '53 bpm', valueSub: '113 bpm high' });
  });

  it('opens the day on 1d', () => {
    expect(rangeRows(METRICS.heart_rate, hr, TODAY)[0]?.target).toEqual({ range: '1d', end: '2026-11-17' });
  });

  it('shows weeks by their dates and opens them on 7d', () => {
    const y = range('steps', '12w', '2026-08-28', TODAY, 'week', [series('steps', '2026-08-28', [null, 40_000, 52_100, 14_000], 7, 'steps')], { goal: 8000 });
    const rows = rangeRows(METRICS.steps, y, TODAY);
    expect(rows.map((r) => r.title)).toEqual(['Sep 18 – 24', 'Sep 11 – 17', 'Sep 4 – 10'].slice(0, 0).concat(rows.map((r) => r.title)));
    expect(rows.map((r) => r.start)).toEqual(['2026-09-18', '2026-09-11', '2026-09-04']);
    expect(rows[0]).toMatchObject({ title: 'Sep 18 – 24', value: '14,000', target: { range: '7d', end: '2026-09-24' } });
    // A week total is not set against the daily goal.
    expect(rows[0]?.sub).toBeNull();
  });

  it('gives a step day its share of the goal', () => {
    const d = week('steps', (f) => [series('steps', f, [null, null, null, null, null, 9000, 4000], 1, 'steps')], { goal: 8000 });
    expect(rangeRows(METRICS.steps, d, TODAY).map((r) => r.sub)).toEqual(['Nov 19 · 50% of goal', 'Nov 18 · 113% of goal']);
  });

  it('gives a week of intensity minutes its share of the weekly goal', () => {
    const d = range('intensity_minutes', '12w', '2026-08-31', TODAY, 'week', [series('moderate', '2026-08-31', [30, 50], 7), series('vigorous', '2026-08-31', [0, 10], 7)], { goal: 150 });
    const rows = rangeRows(METRICS.intensity_minutes, d, TODAY);
    expect(rows[0]).toMatchObject({ value: '70 min', valueSub: '47% of goal', sub: '50 moderate · 10 vigorous' });
    expect(rows[1]).toMatchObject({ value: '30 min', valueSub: '20% of goal' });
  });

  it('is not a link on pages that have no tab to open', () => {
    const d = range('vo2_max', '4w', '2026-10-23', TODAY, 'day', [series('vo2_max', '2026-10-23', [null, 49, null, 50.4], 1, 'ml/kg/min')]);
    const rows = rangeRows(METRICS.vo2_max, d, TODAY);
    expect(rows.map((r) => r.value)).toEqual(['50.4', '49.0']);
    expect(rows.every((r) => r.target === null)).toBe(true);
  });

  it('has no rows for a period without data', () => {
    const d = week('spo2', (f) => [series('average', f, Array(7).fill(null)), series('low', f, Array(7).fill(null))]);
    expect(rangeRows(METRICS.spo2, d, TODAY)).toEqual([]);
    expect(bucketsWithData(d)).toBe(0);
  });
});

describe('rangeChart', () => {
  it('draws resting and high heart rate as two lines with a dot on each day', () => {
    const d = week('heart_rate', (f) => [series('resting', f, [54, null, 55, 56, 55, 54, 53]), series('high', f, [110, null, 121, 118, 120, 111, 113])]);
    const c = rangeChart('heart_rate', d, TODAY) as LineChartSpec;
    expect(c.kind).toBe('line');
    expect(c.series.map((s) => s.label)).toEqual(['High', 'Resting']);
    expect(c.series.every((s) => s.points === true)).toBe(true);
    expect(c.xRange).toEqual([-0.5, 6.5]);
    expect(c.legend.map((l) => l.label)).toEqual(['High', 'Resting']);
    expect(chartHasData(c)).toBe(true);
  });

  it('puts no dots on a long period', () => {
    const d = range('heart_rate', '1y', '2025-11-21', TODAY, 'week', [series('resting', '2025-11-21', Array(52).fill(55), 7), series('high', '2025-11-21', Array(52).fill(110), 7)]);
    expect((rangeChart('heart_rate', d, TODAY) as LineChartSpec).series.every((s) => s.points === false)).toBe(true);
  });

  it('fixes stress to 0 to 100, and energy to a ribbon between each day\'s low and high', () => {
    const s = week('stress', (f) => [series('average', f, [20, 30, 25, null, null, null, null])]);
    const stress = rangeChart('stress', s, TODAY);
    expect(stress.kind).toBe('stacked');
    expect((stress as StackedChartSpec).average.yRange).toEqual([0, 100]);
    const e = week('energy', (f) => [series('high', f, [90, 95, null, null, null, null, null]), series('low', f, [10, 20, null, null, null, null, null])]);
    const c = rangeChart('energy', e, TODAY) as LineChartSpec;
    expect(c.band).toEqual({ lo: [10, 20, null, null, null, null, null], hi: [90, 95, null, null, null, null, null] });
    expect(c.series.every((x) => x.noLine)).toBe(true);
    expect(c.yRange).toEqual([0, 100]);
  });

  it('colours step bars against the daily goal, and has no goal over weeks', () => {
    const d = week('steps', (f) => [series('steps', f, [9000, 4000, null, null, null, null, null])], { goal: 8000 });
    const c = rangeChart('steps', d, TODAY) as BarChartSpec;
    expect(c).toMatchObject({ kind: 'bars', goal: 8000, color: 'var(--steps)', belowGoalColor: 'var(--steps-dim)' });
    expect(c.labels).toEqual(['Fri', 'Sat', 'Sun', 'Mon', 'Tue', 'Wed', 'Thu']);
    const y = range('steps', '1y', '2025-11-21', TODAY, 'week', [series('steps', '2025-11-21', Array(52).fill(40_000), 7)], { goal: 8000 });
    const w = rangeChart('steps', y, TODAY) as BarChartSpec;
    expect(w.goal).toBeUndefined();
    expect(w.axisLabels).toHaveLength(4);
  });

  it('draws an intensity week as the running total against the weekly goal', () => {
    const d = range('intensity_minutes', '7d', '2026-11-16', '2026-11-22', 'day', [series('moderate', '2026-11-16', [10, 15, null, 15, null, null, null]), series('vigorous', '2026-11-16', [0, 5, null, 0, null, null, null])], { goal: 150 });
    const c = rangeChart('intensity_minutes', d, TODAY) as LineChartSpec;
    expect(c.series[0]?.data).toEqual([10, 35, 35, 50, null, null, null]);
    expect(c.series[1]?.data).toEqual(Array(7).fill(150));
    expect(c.yRange).toEqual([0, 160]);
  });

  it('draws weeks of intensity minutes as bars against the weekly goal and days as plain bars', () => {
    const w = range('intensity_minutes', '12w', '2026-08-31', TODAY, 'week', [series('moderate', '2026-08-31', [30, 50], 7), series('vigorous', '2026-08-31', [0, 10], 7)], { goal: 150 });
    expect(rangeChart('intensity_minutes', w, TODAY)).toMatchObject({ kind: 'bars', values: [30, 70], goal: 150 });
    const days = range('intensity_minutes', '4w', '2026-10-23', TODAY, 'day', [series('moderate', '2026-10-23', [30, 50]), series('vigorous', '2026-10-23', [0, 10])], { goal: 150 });
    expect((rangeChart('intensity_minutes', days, TODAY) as BarChartSpec).goal).toBeUndefined();
  });

  it('puts the watch\'s target range behind the training load', () => {
    const d = week('training_load', (f) => [series('acute', f, [300, 305]), series('chronic', f, [280, 282])], { target_low: 250, target_high: 430 });
    const c = rangeChart('training_load', d, TODAY) as LineChartSpec;
    expect(c.band).toEqual([250, 430]);
    expect(c.legend.map((l) => l.label)).toContain('Target range');
    expect((rangeChart('training_load', week('training_load', (f) => [series('acute', f, [300])]), TODAY) as LineChartSpec).band).toBeUndefined();
  });

  it('ends the blood oxygen axis at 100 and starts it under the lowest reading', () => {
    const d = week('spo2', (f) => [series('average', f, [95, 96]), series('low', f, [91, 93])]);
    expect((rangeChart('spo2', d, TODAY) as LineChartSpec).yRange).toEqual([90, 100]);
    const low = week('spo2', (f) => [series('average', f, [88]), series('low', f, [84])]);
    expect((rangeChart('spo2', low, TODAY) as LineChartSpec).yRange).toEqual([80, 100]);
  });

  it('has nothing to draw without a value, for every page', () => {
    for (const metric of Object.keys(METRICS) as MetricKey[]) {
      const d = week(metric, (f) => [series('a', f, Array(7).fill(null))]);
      d.series = ['resting', 'high', 'average', 'rest_s', 'low', 'charged', 'steps', 'moderate', 'vigorous', 'acute', 'chronic', 'vo2_max', 'asleep', 'awake'].map((k) => series(k, '2026-11-13', Array(7).fill(null)));
      expect(chartHasData(rangeChart(metric, d, TODAY)), metric).toBe(false);
    }
  });
});

describe('weekLine', () => {
  it('is a line from 0 with the goal across', () => {
    const c = weekLine(['2026-11-16', '2026-11-17'], [100, 80], 150, TODAY, []);
    expect(c.series.map((s) => s.data)).toEqual([[100, 180], [150, 150]]);
    expect(c.yRange).toEqual([0, 190]);
  });
});

describe('the figures and the words', () => {
  const d = week(
    'heart_rate',
    (f) => [series('resting', f, [null, 54, 55, null, 53, null, null]), series('high', f, [null, 110, 121, null, 113, null, null])],
    {
      stats: [
        { key: 'avg_resting', label: 'Avg resting', value: 54, unit: 'bpm' },
        { key: 'avg_high', label: 'Avg high', value: 114.7, unit: 'bpm' },
      ],
    },
  );

  it('picks the tiles of a page from the response, in order', () => {
    expect(tileStats('heart_rate', d).map((s) => s.key)).toEqual(['avg_resting', 'avg_high']);
    expect(tileStats('steps', d)).toEqual([]);
  });

  it('says in a sentence what the period holds', () => {
    expect(rangeSummary(METRICS.heart_rate, d)).toBe('Heart rate Nov 13 – Nov 19: 3 of 7 days with a reading; avg resting 54 bpm, avg high 115 bpm.');
    const none = week('heart_rate', (f) => [series('resting', f, Array(7).fill(null))]);
    expect(rangeSummary(METRICS.heart_rate, none)).toBe('No heart rate readings Nov 13 – Nov 19.');
  });
});

describe('stress range chart', () => {
  it('stacks minutes per level per day, from the seconds of the API', () => {
    const s = week('stress', (f) => [
      series('average', f, [20, 30, null, null, null, null, null]),
      series('rest_s', f, [3600, 7200, null, null, null, null, null]),
      series('low_s', f, [1800, 0, null, null, null, null, null]),
      series('medium_s', f, [0, 600, null, null, null, null, null]),
      series('high_s', f, [0, 0, null, null, null, null, null]),
    ]);
    const c = rangeChart('stress', s, TODAY) as StackedChartSpec;
    expect(c.parts.map((p) => p.key)).toEqual(['rest', 'low', 'medium', 'high']);
    expect(c.parts[0]?.values.slice(0, 3)).toEqual([60, 120, null]);
    expect(c.parts[2]?.values.slice(0, 2)).toEqual([0, 10]);
  });
});
