import { describe, expect, it } from 'vitest';
import type { SleepNight } from '../../api/generated/SleepNight';
import type { TrendSeries } from '../../api/generated/TrendSeries';
import type { TrendsResponse } from '../../api/generated/TrendsResponse';
import { addDays } from '../../lib/dates';
import {
  axisDates,
  durationSummary,
  hourSeries,
  hrvAside,
  hrvSummary,
  nightBars,
  nightsSummary,
  rangeAxis,
  rhrSummary,
  scoreLabel,
  sleepTicks,
  stageSpans,
  stageSummary,
  stepsPercent,
  stressBuckets,
  trendXY,
} from './model';
import { clock, clockAt, dayStartMs, offsetMin } from './time';

const night = (over: Partial<SleepNight> = {}): SleepNight => ({
  date: '2026-11-18',
  start: '2026-11-18T23:00:00+01:00',
  end: '2026-11-19T07:00:00+01:00',
  duration_s: 27000,
  time_in_bed_s: 28800,
  score: 81,
  score_label: 'good',
  scores: { overall: 81, quality: null, duration: null, recovery: null, deep: null, light: null, rem: null },
  awakenings: 2,
  avg_stress: null,
  stages_available: true,
  deep_s: 5040,
  light_s: 14280,
  rem_s: 6600,
  awake_s: 720,
  stages: [
    { start: '2026-11-18T23:00:00+01:00', end: '2026-11-18T23:30:00+01:00', stage: 'light' },
    { start: '2026-11-18T23:30:00+01:00', end: '2026-11-19T00:30:00+01:00', stage: 'deep' },
    { start: '2026-11-19T00:30:00+01:00', end: '2026-11-19T00:40:00+01:00', stage: 'unmeasurable' },
    { start: '2026-11-19T00:40:00+01:00', end: '2026-11-19T08:00:00+01:00', stage: 'rem' },
  ],
  hrv_ms: null,
  lowest_hr_bpm: null,
  breathing_brpm: null,
  spo2_avg_percent: null,
  spo2_low_percent: null,
  ...over,
});

const series = (values: (number | null)[], from = '2026-10-23'): TrendSeries => {
  const nums = values.filter((v): v is number => v !== null);
  return {
    average: nums.length ? nums.reduce((a, b) => a + b, 0) / nums.length : null,
    min: nums.length ? Math.min(...nums) : null,
    max: nums.length ? Math.max(...nums) : null,
    points: values.map((value, i) => ({ date: addDays(from, i), value })),
  };
};

describe('time helpers read the wall clock of the owner, not the browser', () => {
  it('keeps the written clock time and the offset', () => {
    expect(clock('2026-11-18T23:32:41+01:00')).toBe('23:32');
    expect(clock(null)).toBeNull();
    expect(clock('nonsense')).toBeNull();
    expect(offsetMin('2026-11-18T23:32:41+01:00')).toBe(60);
    expect(offsetMin('2026-11-18T23:32:41-05:30')).toBe(-330);
    expect(offsetMin('2026-11-18T23:32:41Z')).toBe(0);
    expect(clockAt(Date.parse('2026-11-18T22:00:00Z'), 60)).toBe('23:00');
  });
  it('finds local midnight in the zone of a sample', () => {
    expect(dayStartMs('2026-11-18', '2026-11-18T05:00:00+01:00')).toBe(Date.parse('2026-11-17T23:00:00Z'));
  });
});

describe('sleep night', () => {
  it('turns stages into minutes from bedtime and drops unmeasurable ones', () => {
    const { spans, totalMinutes } = stageSpans(night());
    expect(totalMinutes).toBe(480);
    expect(spans).toEqual([
      { stage: 'light', from: 0, to: 30 },
      { stage: 'deep', from: 30, to: 90 },
      { stage: 'rem', from: 100, to: 480 }, // clamped to the end of the night
    ]);
  });
  it('labels the timeline in local clock time', () => {
    expect(sleepTicks(night())).toEqual(['23:00', '01:00', '03:00', '05:00', '07:00']);
  });
  it('summarises the stages in one line, and says so when they are missing', () => {
    expect(stageSummary(night())).toBe('Sleep stages through the night: deep sleep 1h 24m, light sleep 3h 58m, REM 1h 50m, awake 12m, 2 awakenings.');
    expect(stageSummary(night({ stages_available: false, stages: [] }))).toMatch(/not recorded/);
  });
  it('capitalises the score label and keeps null', () => {
    expect(scoreLabel('good')).toBe('Good');
    expect(scoreLabel(null)).toBeNull();
  });
});

describe('last nights', () => {
  const n = (date: string, h: number) => ({ date, start: '', end: '', duration_s: h * 3600, score: 80, deep_s: null, light_s: null, rem_s: null, awake_s: null });
  it('leaves a gap for a night without data instead of a zero', () => {
    const bars = nightBars([n('2026-11-15', 7), n('2026-11-17', 6), n('2026-11-18', 8)], 5);
    expect(bars.dates).toEqual(['2026-11-14', '2026-11-15', '2026-11-16', '2026-11-17', '2026-11-18']);
    expect(bars.values).toEqual([null, 420, null, 360, 480]);
    expect(bars.labels[4]).toBe('Wed');
  });
  it('has nothing to draw without nights', () => {
    expect(nightBars([], 7).dates).toEqual([]);
    expect(nightsSummary([], 7, null)).toBeNull();
  });
  it('summarises the period', () => {
    expect(nightsSummary([n('2026-11-17', 6), n('2026-11-18', 8)], 7, 77.6)).toBe('Sleep averaged 7h 0m over 2 of the last 7 nights, between 6h 0m and 8h 0m, average score 78.');
  });
  it('picks evenly spaced axis dates', () => {
    expect(axisDates(['2026-11-01', '2026-11-02', '2026-11-03', '2026-11-04'], 4)).toEqual(['1 Nov', '2 Nov', '3 Nov', '4 Nov']);
  });
});

describe('day series', () => {
  it('places samples in hours since local midnight with empty ends', () => {
    const s = hourSeries('2026-11-18', [
      { time: '2026-11-18T06:00:00+01:00', value: 50 },
      { time: '2026-11-18T18:00:00+01:00', value: null },
    ]);
    expect(s).toEqual({ x: [0, 6, 18, 24], y: [null, 50, null, null] });
  });
  it('is unavailable without any value', () => {
    expect(hourSeries('2026-11-18', [])).toBeNull();
    expect(hourSeries('2026-11-18', [{ time: '2026-11-18T06:00:00+01:00', value: null }])).toBeNull();
  });
  it('folds stress into 30 minute columns by dominant level', () => {
    const b = stressBuckets('2026-11-18', [
      { time: '2026-11-18T00:00:00+01:00', stress: 10, level: 'rest' },
      { time: '2026-11-18T00:10:00+01:00', stress: 12, level: 'rest' },
      { time: '2026-11-18T00:20:00+01:00', stress: 40, level: 'low' },
      { time: '2026-11-18T00:30:00+01:00', stress: 30, level: 'low' },
      { time: '2026-11-18T00:40:00+01:00', stress: 50, level: 'medium' },
      { time: '2026-11-18T01:00:00+01:00', stress: null, level: 'activity' },
    ]);
    expect(b).toHaveLength(48);
    expect(b[0]).toEqual({ level: 'rest', value: null });
    expect(b[1]).toEqual({ level: 'low', value: 40 }); // low and medium tie; the first seen wins, mean of both is 40
    expect(b[2]).toEqual({ level: 'activity', value: null });
    expect(b[5]).toEqual({ level: 'low', value: null }); // no samples: empty, not zero
  });
  it('computes the share of the step goal only when steps exist', () => {
    expect(stepsPercent(4000, 8000)).toBe(50);
    expect(stepsPercent(null, 8000)).toBeNull();
  });
});

describe('trends', () => {
  const trends = (hrv: TrendSeries, lo: number | null, hi: number | null): TrendsResponse['hrv'] => ({ series: hrv, baseline_low_ms: lo, baseline_high_ms: hi });

  it('counts the nights inside the personal range', () => {
    // 28 nights, 25 inside 55 to 70
    const values = Array.from({ length: 28 }, (_, i) => (i < 3 ? 40 : 61));
    const t = trends({ ...series(values), average: 61 }, 55, 70);
    expect(hrvSummary(t)).toBe('HRV averaged 61 ms, within your range on 25 of 28 nights.');
    expect(hrvAside(t)).toBe('Avg 61 ms · range 55 to 70');
  });
  it('says so when the watch has no personal range yet', () => {
    const t = trends(series([60, 62]), null, null);
    expect(hrvSummary(t)).toMatch(/not set a personal range/);
    expect(hrvAside(t)).toBe('Avg 61 ms');
  });
  it('is unavailable without readings', () => {
    const t = trends(series([null, null]), 55, 70);
    expect(hrvSummary(t)).toBe('No overnight HRV readings in this range.');
    expect(hrvAside(t)).toBeNull();
    expect(rhrSummary(series([]))).toBe('No resting heart rate readings in this range.');
    expect(durationSummary(series([]))).toBe('No sleep duration readings in this range.');
  });
  it('keeps real gaps in time: x is days since the start, ends are empty anchors', () => {
    const xy = trendXY(series([60, null, 62], '2026-10-23'), '2026-10-23', '2026-11-19');
    expect(xy).toEqual({ x: [0, 0, 2, 27], y: [null, 60, 62, null] });
    expect(trendXY(series([null]), '2026-10-23', '2026-11-19')).toBeNull();
  });
  it('converts units for the chart', () => {
    const xy = trendXY({ ...series([7200]), points: [{ date: '2026-10-23', value: 7200 }] }, '2026-10-23', '2026-11-19', 1 / 3600);
    expect(xy?.y[1]).toBe(2);
  });
  it('labels the range axis', () => {
    expect(rangeAxis({ from: '2026-10-23', to: '2026-11-19' })).toEqual(['23 Oct', '1 Nov', '10 Nov', '19 Nov']);
    expect(rangeAxis({ from: '2025-11-20', to: '2026-11-19' })[0]).toBe('Nov 2025');
  });
});
