// Pure mapping from the range endpoints (training status, HRV, sleep history) to what the pages
// draw. Missing values stay null (rendered as "unavailable"), never 0. Status names and kinds come
// from the server; nothing here names a status code.

import type { BandSeries, ChartMarker, XLabel } from '../../components';
import { statusName } from '../../lib/format';
import type { HrvDay } from '../../api/generated/HrvDay';
import type { HrvRange } from '../../api/generated/HrvRange';
import type { HrvStatusKind } from '../../api/generated/HrvStatusKind';
import type { SleepBucket } from '../../api/generated/SleepBucket';
import type { SleepHistory } from '../../api/generated/SleepHistory';
import type { StatusKind } from '../../api/generated/StatusKind';
import type { StatusRun } from '../../api/generated/StatusRun';
import type { TrainingStatusRange } from '../../api/generated/TrainingStatusRange';
import { addDays, fmtDay, fmtMonthDay, fmtMonthLetter, fmtMonthShort, fmtPeriod, fmtRunDates } from '../../lib/dates';
import { fmtFixed, fmtInt, isMissing } from '../../lib/format';
import { fmtSeconds } from './model';
import { daysBetween } from './time';

// ---- training status ------------------------------------------------------------------------------

/** Colour of a status in the timeline, the legend and the list (see the tokens). */
export const STATUS_COLOR: Record<StatusKind, string> = {
  recovery: 'var(--status-recovery)',
  strained: 'var(--status-strained)',
  maintaining: 'var(--status-maintaining)',
  productive: 'var(--status-productive)',
  detraining: 'var(--status-detraining)',
  none: 'var(--status-none)',
  unknown: 'var(--status-none)',
};

/** Where a run of equal status is, in words: "Since Nov 17" for the status now, else the dates. */
export function runDates(run: Pick<StatusRun, 'from' | 'to' | 'ongoing' | 'starts_before_range'>): string {
  if (run.ongoing) return `Since ${fmtMonthDay(run.from)}${run.starts_before_range ? ' or earlier' : ''}`;
  return `${fmtRunDates(run.from, run.to)}${run.starts_before_range ? ' or earlier' : ''}`;
}

/** The timeline in words, for screen readers: "Training status Nov 14 – Nov 20: Recovery since Nov 17, Strained Nov 15 – 16." */
export function timelineSummary(t: Pick<TrainingStatusRange, 'from' | 'to' | 'runs' | 'days'>): string {
  const period = fmtPeriod(t.from, t.to);
  if (t.runs.length === 0) return `Training status ${period}: no status recorded.`;
  const parts = t.runs.map((r) => `${statusName(r.status.label) ?? r.status.label} ${runDates(r).replace(/^Since/, 'since')}`);
  const known = t.days.filter((d) => d.status !== null).length;
  return `Training status ${period}: ${parts.join(', ')}. ${known} of ${t.days.length} days have a status.`;
}

// ---- HRV --------------------------------------------------------------------------------------------

export const HRV_MARKER: Record<HrvStatusKind, ChartMarker> = {
  balanced: { shape: 'dot', color: 'var(--hrv-balanced)' },
  unbalanced: { shape: 'square', color: 'var(--hrv-unbalanced)' },
  low: { shape: 'triangle', color: 'var(--hrv-low)' },
  poor: { shape: 'triangle', color: 'var(--hrv-low)' },
};
/** A reading whose status is missing or not one of the four names. */
const NO_STATUS_MARKER: ChartMarker = { shape: 'dot', color: 'var(--status-none)' };

export type HrvMode = 'weekly' | 'overnight';

export const hrvValue = (d: HrvDay, mode: HrvMode): number | null => (mode === 'weekly' ? d.weekly_avg_ms : d.last_night_ms);

export const hrvHasData = (d: HrvDay): boolean => d.status_code !== null || d.weekly_avg_ms !== null || d.last_night_ms !== null || d.baseline_low_ms !== null;

/**
 * The balanced range per day. The watch reports it with each summary; a day without one keeps the
 * range of the previous reading as long as a later reading exists (no range is drawn before the
 * first or after the last one).
 */
export function heldBaseline(days: HrvDay[]): BandSeries {
  const lo: (number | null)[] = days.map(() => null);
  const hi: (number | null)[] = days.map(() => null);
  const known = days.flatMap((d, i) => (d.baseline_low_ms !== null && d.baseline_high_ms !== null ? [i] : []));
  const first = known[0];
  const last = known[known.length - 1];
  if (first === undefined || last === undefined) return { lo, hi };
  let cur: number | null = null;
  for (let i = first; i <= last; i++) {
    if (known.includes(i)) cur = i;
    lo[i] = days[cur as number]?.baseline_low_ms ?? null;
    hi[i] = days[cur as number]?.baseline_high_ms ?? null;
  }
  return { lo, hi };
}

export interface HrvChartData {
  /** Day index since the start of the period. */
  x: number[];
  y: (number | null)[];
  markers: (ChartMarker | null)[];
  band: BandSeries | null;
  /** Days with a value to plot. */
  count: number;
}

export function hrvChart(days: HrvDay[], mode: HrvMode): HrvChartData {
  const y = days.map((d) => hrvValue(d, mode));
  const band = heldBaseline(days);
  return {
    x: days.map((_, i) => i),
    y,
    markers: days.map((d, i) => (y[i] === null ? null : d.status ? HRV_MARKER[d.status] : NO_STATUS_MARKER)),
    band: band.lo.some((v) => v !== null) ? band : null,
    count: y.filter((v) => v !== null).length,
  };
}

/** The chart in words. */
export function hrvChartSummary(t: Pick<HrvRange, 'days' | 'from' | 'to'>, mode: HrvMode): string {
  const name = mode === 'weekly' ? '7-day average HRV' : 'Overnight HRV';
  const withValue = t.days.filter((d) => hrvValue(d, mode) !== null);
  if (withValue.length === 0) return `No ${name} readings ${fmtPeriod(t.from, t.to)}.`;
  const vals = withValue.map((d) => hrvValue(d, mode) as number);
  const last = withValue[withValue.length - 1] as HrvDay;
  const status = last.status_label ? ` (${last.status_label})` : '';
  const range = last.baseline_low_ms !== null && last.baseline_high_ms !== null ? ` Balanced range ${fmtInt(last.baseline_low_ms)} to ${fmtInt(last.baseline_high_ms)} ms.` : '';
  const lo = fmtInt(Math.min(...vals));
  const hi = fmtInt(Math.max(...vals));
  const spread = lo === hi ? (withValue.length === 1 ? `${lo} ms` : `${lo} ms every day`) : `between ${lo} and ${hi} ms`;
  return `${name} ${fmtPeriod(t.from, t.to)}: ${withValue.length} of ${t.days.length} days with a reading, ${spread}, latest ${fmtInt(hrvValue(last, mode))} ms on ${fmtMonthDay(last.date)}${status}.${range}`;
}

export function baselineText(low: number | null, high: number | null): string | null {
  const a = fmtInt(low);
  const b = fmtInt(high);
  return a === null || b === null ? null : `${a} to ${b} ms`;
}

/** "55–70" for a tile whose unit is under the number. */
export function baselineShort(low: number | null, high: number | null): string | null {
  const a = fmtInt(low);
  const b = fmtInt(high);
  return a === null || b === null ? null : `${a}–${b}`;
}

/** Day offsets (0 is `from`) that get a date label under a chart over `count` days. */
function dayAxisOffsets(count: number): number[] {
  if (count < 2) return [0];
  // Up to four days: a label for each day (spacing four labels over three days would repeat one).
  if (count <= 4) return Array.from({ length: count }, (_, i) => i);
  return [0, 1, 2, 3].map((i) => Math.round((i * (count - 1)) / 3));
}

/** Four evenly spaced date labels under a chart over `days` days from `from`, each at its day's x. */
export function dayAxisLabels(from: string, count: number): XLabel[] {
  return dayAxisOffsets(count).map((i) => ({ value: i, text: fmtMonthDay(addDays(from, i)) }));
}

/** The text of `dayAxisLabels`. */
export function dayAxis(from: string, count: number): string[] {
  return dayAxisLabels(from, count).map((l) => l.text);
}

// ---- sleep history ----------------------------------------------------------------------------------

export interface SleepHistoryChart {
  /** Bucket index. */
  x: number[];
  score: (number | null)[];
  /** Minutes asleep per bucket. */
  minutes: (number | null)[];
  /** Label under each bar (short weekday, month) or null when only a few date labels fit. */
  labels: string[] | null;
  axis: string[];
  /** The same labels with the bucket (x) each sits under. */
  axisLabels: XLabel[];
  /** Buckets with a night. */
  count: number;
}

const WEEKDAY = (day: string) => fmtDay(day, { weekday: 'short' });

/** `n` evenly spaced indexes of a list of `length` (first and last included). */
function spreadIndexes(length: number, n: number): number[] {
  if (length <= n) return Array.from({ length }, (_, i) => i);
  return Array.from({ length: n }, (_, i) => Math.round((i * (length - 1)) / (n - 1)));
}

export function sleepChart(h: Pick<SleepHistory, 'buckets' | 'group' | 'from'>): SleepHistoryChart {
  const b = h.buckets;
  const labels = h.group === 'month' ? b.map((x) => fmtMonthLetter(x.from)) : h.group === 'day' && b.length <= 7 ? b.map((x) => WEEKDAY(x.from)) : null;
  const axisLabels: XLabel[] =
    h.group === 'month'
      ? spreadIndexes(b.length, 4).map((i) => ({ value: i, text: fmtMonthShort((b[i] as SleepBucket).from) }))
      : h.group === 'day' && b.length <= 7
        ? (labels ?? []).map((text, i) => ({ value: i, text }))
        : dayAxisLabels(b[0]?.from ?? h.from, b.length);
  return {
    x: b.map((_, i) => i),
    score: b.map((x) => x.avg_score),
    minutes: b.map((x) => (isMissing(x.avg_duration_s) ? null : x.avg_duration_s / 60)),
    labels,
    axis: axisLabels.map((l) => l.text),
    axisLabels,
    count: b.filter((x) => x.nights > 0).length,
  };
}

/** What a bucket is called in the table and the text: "Nov 12", "Nov 9 – 15", "November". */
export function bucketName(b: Pick<SleepBucket, 'from' | 'to'>, group: string): string {
  if (group === 'month') return fmtDay(b.from, { month: 'long', year: 'numeric' });
  return fmtRunDates(b.from, b.to);
}

export function sleepStatLabels(group: string): { avg: string; high: string; low: string } {
  // A day bucket is one night, so "average highest" would only repeat the average: say highest.
  return group === 'day' ? { avg: 'Avg score', high: 'Highest', low: 'Lowest' } : { avg: 'Avg score', high: 'Avg highest', low: 'Avg lowest' };
}

/** The long-night limit as the pages write it: "8 h". */
const longNight = (h: Pick<SleepHistory, 'long_night_s'>): string => `${Math.round(h.long_night_s / 3600)} h`;

export function sleepSummary(h: SleepHistory): string {
  const s = h.stats;
  const period = fmtPeriod(h.from, h.to);
  if (s.nights === 0) return `No sleep recorded ${period}.`;
  const score = s.avg_score === null ? 'no scores' : `average score ${fmtInt(s.avg_score)}, from ${fmtInt(s.lowest_score)} to ${fmtInt(s.highest_score)}`;
  const long = s.long_nights > 0 ? `, ${s.long_nights} night${s.long_nights === 1 ? '' : 's'} of ${longNight(h)} or more` : '';
  return `Sleep ${period}: ${s.nights} night${s.nights === 1 ? '' : 's'}, ${score}, average time asleep ${fmtSeconds(s.avg_duration_s) ?? 'unavailable'}${long}.`;
}

export function scoreChartSummary(h: SleepHistory): string {
  const withScore = h.buckets.filter((b) => b.avg_score !== null);
  if (withScore.length === 0) return `No sleep scores ${fmtPeriod(h.from, h.to)}.`;
  const unit = h.group === 'month' ? 'month' : h.group === 'week' ? 'week' : 'night';
  const parts =
    withScore.length <= 7 ? withScore.map((b) => `${bucketName(b, h.group)} ${fmtFixed(b.avg_score, h.group === 'day' ? 0 : 1)}`).join(', ') : `${withScore.length} ${unit}s`;
  return `Sleep score ${fmtPeriod(h.from, h.to)}: ${parts}; average ${fmtInt(h.stats.avg_score)}.`;
}

export function durationChartSummary(h: SleepHistory): string {
  const withTime = h.buckets.filter((b) => b.avg_duration_s !== null);
  if (withTime.length === 0) return `No time asleep ${fmtPeriod(h.from, h.to)}.`;
  const unit = h.group === 'day' ? 'night' : h.group;
  const parts = withTime.length <= 7 ? withTime.map((b) => `${bucketName(b, h.group)} ${fmtSeconds(b.avg_duration_s)}`).join(', ') : `${withTime.length} ${unit}s`;
  return `Time asleep per ${unit} ${fmtPeriod(h.from, h.to)}: ${parts}; average ${fmtSeconds(h.stats.avg_duration_s)}. Bars of ${longNight(h)} or more are green.`;
}

/** Whole days in a period, for the chart axes. */
export const periodDays = (from: string, to: string): number => daysBetween(from, to) + 1;
