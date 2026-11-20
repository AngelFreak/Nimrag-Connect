import { describe, expect, it } from 'vitest';
import type { HrvDay } from '../../api/generated/HrvDay';
import type { SleepBucket } from '../../api/generated/SleepBucket';
import type { SleepHistory } from '../../api/generated/SleepHistory';
import type { StatusInfo } from '../../api/generated/StatusInfo';
import type { StatusRun } from '../../api/generated/StatusRun';
import { addDays, fmtDayLong, fmtMonthDay, fmtMonthLetter, fmtMonthShort, fmtPeriod, fmtRunDates, todayIn } from '../../lib/dates';
import { periodWindow } from './period';
import {
  baselineShort,
  baselineText,
  dayAxis,
  heldBaseline,
  hrvChart,
  hrvChartSummary,
  hrvHasData,
  runDates,
  scoreChartSummary,
  sleepChart,
  sleepStatLabels,
  sleepSummary,
  timelineSummary,
} from './ranges';

const info = (label: string, kind: StatusInfo['kind'], code: number | null): StatusInfo => ({ code, label, kind });
const RECOVERY = info('Recovery', 'recovery', 5);
const STRAINED = info('Strained', 'strained', 8);

const run = (status: StatusInfo, from: string, to: string, over: Partial<StatusRun> = {}): StatusRun => ({
  status,
  from,
  to,
  days: 1,
  starts_before_range: false,
  ongoing: false,
  ...over,
});

const hrvDay = (date: string, over: Partial<HrvDay> = {}): HrvDay => ({
  date,
  status_code: null,
  status: null,
  status_label: null,
  weekly_avg_ms: null,
  last_night_ms: null,
  last_night_5min_high_ms: null,
  baseline_low_ms: null,
  baseline_high_ms: null,
  ...over,
});

describe('date helpers', () => {
  it('writes periods month first', () => {
    expect(fmtMonthDay('2026-11-14')).toBe('Nov 14');
    expect(fmtDayLong('2026-11-19')).toBe('Thu, Nov 19');
    expect(fmtMonthShort('2026-09-01')).toBe('Sep');
    expect(fmtMonthLetter('2026-09-01')).toBe('S');
    expect(fmtPeriod('2026-11-14', '2026-11-20')).toBe('Nov 14 – Nov 20');
    expect(fmtPeriod('2026-11-14', '2026-11-14')).toBe('Nov 14');
  });
  it('adds the years when a period crosses a year or is long', () => {
    expect(fmtPeriod('2025-12-30', '2026-01-05')).toBe('Dec 30, 2025 – Jan 5, 2026');
    expect(fmtPeriod('2025-11-21', '2026-11-20')).toBe('Nov 21, 2025 – Nov 20, 2026');
  });
  it('writes the dates of a run of days', () => {
    expect(fmtRunDates('2026-11-15', '2026-11-16')).toBe('Nov 15 – 16');
    expect(fmtRunDates('2026-11-15', '2026-11-15')).toBe('Nov 15');
    expect(fmtRunDates('2026-10-30', '2026-11-02')).toBe('Oct 30 – Nov 2');
  });
  it('takes today from the zone, not from the browser', () => {
    expect(todayIn('Pacific/Kiritimati')).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(todayIn(null)).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(todayIn('Not/AZone')).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});

describe('periodWindow', () => {
  it('ends today by default and counts the days back', () => {
    expect(periodWindow(null, 7, '2026-11-20')).toEqual({ from: '2026-11-14', to: '2026-11-20' });
    expect(periodWindow(null, 1, '2026-11-20')).toEqual({ from: '2026-11-20', to: '2026-11-20' });
  });
  it('ends on a given earlier day, never after today, and ignores junk', () => {
    expect(periodWindow('2026-11-13', 7, '2026-11-20')).toEqual({ from: '2026-11-07', to: '2026-11-13' });
    expect(periodWindow('2026-12-01', 7, '2026-11-20').to).toBe('2026-11-20');
    expect(periodWindow('2026-13-45', 7, '2026-11-20').to).toBe('2026-11-20');
    expect(periodWindow('nonsense', 28, '2026-11-20')).toEqual({ from: '2026-10-24', to: '2026-11-20' });
  });
});

describe('training status text', () => {
  it('says since for the status now and the dates for the rest', () => {
    expect(runDates(run(RECOVERY, '2026-11-17', '2026-11-20', { ongoing: true }))).toBe('Since Nov 17');
    expect(runDates(run(STRAINED, '2026-11-15', '2026-11-16'))).toBe('Nov 15 – 16');
    expect(runDates(run(RECOVERY, '2026-11-14', '2026-11-14', { starts_before_range: true }))).toBe('Nov 14 or earlier');
    expect(runDates(run(RECOVERY, '2026-11-14', '2026-11-20', { ongoing: true, starts_before_range: true }))).toBe('Since Nov 14 or earlier');
  });
  it('describes the timeline for a screen reader', () => {
    const days = ['2026-11-15', '2026-11-16', '2026-11-17'].map((date, i) => ({ date, status: i < 2 ? STRAINED : RECOVERY }));
    const text = timelineSummary({
      from: '2026-11-15',
      to: '2026-11-17',
      days: [...days, { date: '2026-11-18', status: null }],
      runs: [run(RECOVERY, '2026-11-17', '2026-11-17', { ongoing: true }), run(STRAINED, '2026-11-15', '2026-11-16')],
    });
    expect(text).toBe('Training status Nov 15 – Nov 17: Recovery since Nov 17, Strained Nov 15 – 16. 3 of 4 days have a status.');
  });
  it('says so when no status was recorded', () => {
    expect(timelineSummary({ from: '2026-11-15', to: '2026-11-17', days: [], runs: [] })).toBe('Training status Nov 15 – Nov 17: no status recorded.');
  });
});

describe('HRV chart data', () => {
  const days = [
    hrvDay('2026-11-14', { status: 'balanced', status_code: 4, status_label: 'Balanced', weekly_avg_ms: 60, last_night_ms: 58, baseline_low_ms: 55, baseline_high_ms: 70 }),
    hrvDay('2026-11-15'),
    hrvDay('2026-11-16', { status: 'unbalanced', status_code: 3, status_label: 'Unbalanced', weekly_avg_ms: 52, baseline_low_ms: 56, baseline_high_ms: 71 }),
    hrvDay('2026-11-17'),
  ];
  it('recognises a day with something to show', () => {
    expect(hrvHasData(days[0] as HrvDay)).toBe(true);
    expect(hrvHasData(days[1] as HrvDay)).toBe(false);
  });
  it('holds the balanced range over a gap, but not before the first or after the last', () => {
    const band = heldBaseline(days);
    expect(band.lo).toEqual([55, 55, 56, null]);
    expect(band.hi).toEqual([70, 70, 71, null]);
  });
  it('gives no band at all when no day has a range', () => {
    expect(hrvChart([hrvDay('2026-11-14', { weekly_avg_ms: 60 })], 'weekly').band).toBeNull();
  });
  it('plots the chosen value with a marker per status and a gap where it is missing', () => {
    const weekly = hrvChart(days, 'weekly');
    expect(weekly.y).toEqual([60, null, 52, null]);
    expect(weekly.markers.map((m) => m?.shape ?? null)).toEqual(['dot', null, 'square', null]);
    expect(weekly.count).toBe(2);
    const overnight = hrvChart(days, 'overnight');
    expect(overnight.y).toEqual([58, null, null, null]);
    expect(overnight.count).toBe(1);
  });
  it('draws low and poor as triangles and a missing status as a grey dot', () => {
    const c = hrvChart(
      [hrvDay('2026-11-14', { status: 'low', weekly_avg_ms: 40 }), hrvDay('2026-11-15', { status: 'poor', weekly_avg_ms: 38 }), hrvDay('2026-11-16', { weekly_avg_ms: 50 })],
      'weekly',
    );
    expect(c.markers.map((m) => m?.shape)).toEqual(['triangle', 'triangle', 'dot']);
  });
  it('summarises the chart, also with no reading', () => {
    const s = hrvChartSummary({ from: '2026-11-14', to: '2026-11-17', days }, 'weekly');
    expect(s).toContain('2 of 4 days with a reading');
    expect(s).toContain('between 52 and 60 ms');
    expect(s).toContain('latest 52 ms on Nov 16 (Unbalanced)');
    expect(s).toContain('Balanced range 56 to 71 ms');
    expect(hrvChartSummary({ from: '2026-11-14', to: '2026-11-14', days: [hrvDay('2026-11-14')] }, 'overnight')).toBe('No Overnight HRV readings Nov 14.');
  });
  it('writes the range with and without its unit', () => {
    expect(baselineText(55, 70)).toBe('55 to 70 ms');
    expect(baselineShort(55, 70)).toBe('55–70');
    expect(baselineText(null, 70)).toBeNull();
    expect(baselineShort(55, null)).toBeNull();
  });
  it('spaces the date labels under a chart', () => {
    expect(dayAxis('2026-11-14', 1)).toEqual(['Nov 14']);
    expect(dayAxis('2026-11-14', 3)).toEqual(['Nov 14', 'Nov 15', 'Nov 16']);
    expect(dayAxis('2026-11-14', 7)).toEqual(['Nov 14', 'Nov 16', 'Nov 18', 'Nov 20']);
  });
});

describe('sleep history text and chart data', () => {
  const bucket = (from: string, over: Partial<SleepBucket> = {}): SleepBucket => ({
    from,
    to: from,
    nights: 1,
    avg_score: 80,
    high_score: 80,
    low_score: 80,
    avg_duration_s: 8 * 3600,
    ...over,
  });
  const history = (buckets: SleepBucket[], group: string, over: Partial<SleepHistory['stats']> = {}): SleepHistory => ({
    sample: false,
    from: buckets[0]?.from ?? '2026-11-14',
    to: buckets[buckets.length - 1]?.to ?? '2026-11-20',
    today: '2026-11-20',
    group,
    long_night_s: 8 * 3600,
    buckets,
    stats: {
      nights: buckets.filter((b) => b.nights > 0).length,
      scored_nights: 2,
      avg_score: 75,
      highest_score: 80,
      lowest_score: 70,
      avg_highest: 80,
      avg_lowest: 70,
      avg_duration_s: 7.5 * 3600,
      long_nights: 1,
      ...over,
    },
  });
  it('labels a week by weekday and a year by the first letter of its months', () => {
    const week = sleepChart(
      history(
        ['2026-11-14', '2026-11-15', '2026-11-16'].map((d) => bucket(d)),
        'day',
      ),
    );
    expect(week.labels).toEqual(['Sat', 'Sun', 'Mon']);
    const year = sleepChart(
      history(
        ['2026-01-01', '2026-02-01', '2026-03-01'].map((d) => bucket(d, { nights: 0, avg_score: null, avg_duration_s: null })),
        'month',
      ),
    );
    expect(year.labels).toEqual(['J', 'F', 'M']);
    expect(year.score).toEqual([null, null, null]);
    expect(year.minutes).toEqual([null, null, null]);
    expect(year.count).toBe(0);
  });
  it('uses date labels for four weeks of nights and keeps missing nights as gaps', () => {
    const days = Array.from({ length: 28 }, (_, i) => bucket(addDays('2026-10-24', i), i === 3 ? { nights: 0, avg_score: null, avg_duration_s: null } : {}));
    const c = sleepChart(history(days, 'day'));
    expect(c.labels).toBeNull();
    expect(c.axis).toHaveLength(4);
    expect(c.score[3]).toBeNull();
    expect(c.minutes[0]).toBe(480);
  });
  it('names the stats by group', () => {
    expect(sleepStatLabels('day')).toEqual({ avg: 'Avg score', high: 'Highest', low: 'Lowest' });
    expect(sleepStatLabels('month')).toEqual({ avg: 'Avg score', high: 'Avg highest', low: 'Avg lowest' });
  });
  it('summarises with one night, few nights and none', () => {
    const one = history([bucket('2026-11-20', { avg_score: 77 })], 'day', { nights: 1, long_nights: 1, avg_score: 77, highest_score: 77, lowest_score: 77 });
    expect(sleepSummary(one)).toBe('Sleep Nov 20: 1 night, average score 77, from 77 to 77, average time asleep 7h 30m, 1 night of 8 h or more.');
    expect(scoreChartSummary(one)).toBe('Sleep score Nov 20: Nov 20 77; average 77.');
    const none = history([bucket('2026-11-20', { nights: 0, avg_score: null, avg_duration_s: null })], 'day', { nights: 0, avg_score: null });
    expect(sleepSummary(none)).toBe('No sleep recorded Nov 20.');
    expect(scoreChartSummary(none)).toBe('No sleep scores Nov 20.');
  });
});
