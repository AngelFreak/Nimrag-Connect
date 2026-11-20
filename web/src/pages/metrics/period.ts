// Which metrics have a detail page, which periods each one offers, and the period maths: the window
// a tab shows, where the arrows go and where a tapped row leads. Pure, so the rules are tested
// without a page. Days are local days ("YYYY-MM-DD"); a period is named by the day it ends on.

import { addDays, isIsoDay } from '../../lib/dates';

export type MetricKey = 'heart_rate' | 'stress' | 'energy' | 'steps' | 'intensity_minutes' | 'training_load' | 'vo2_max' | 'respiration' | 'spo2';

/** The period tabs. `1d` is one day (GET /api/days); the others are GET /api/metrics ranges. */
export type RangeKey = '1d' | '7d' | '4w' | '12w' | '6m' | '1y';
export type ServerRange = Exclude<RangeKey, '1d'>;

/** Length of a window in days: 7d is 7 days, 4w 28, 12w 84, 6m 182, 1y 364 (52 weeks). */
export const RANGE_DAYS: Record<RangeKey, number> = { '1d': 1, '7d': 7, '4w': 28, '12w': 84, '6m': 182, '1y': 364 };

export interface MetricDef {
  key: MetricKey;
  /** Page title. */
  title: string;
  /** The tabs, in order. */
  ranges: readonly RangeKey[];
  /** The tab a link without `range` opens. */
  defaultRange: RangeKey;
}

export const METRICS: Record<MetricKey, MetricDef> = {
  heart_rate: { key: 'heart_rate', title: 'Heart rate', ranges: ['1d', '7d', '4w', '1y'], defaultRange: '1d' },
  stress: { key: 'stress', title: 'Stress', ranges: ['1d', '7d', '4w', '1y'], defaultRange: '1d' },
  energy: { key: 'energy', title: 'Energy', ranges: ['1d', '7d', '4w'], defaultRange: '1d' },
  steps: { key: 'steps', title: 'Steps', ranges: ['1d', '7d', '4w', '1y'], defaultRange: '1d' },
  intensity_minutes: { key: 'intensity_minutes', title: 'Intensity minutes', ranges: ['1d', '7d', '4w', '1y'], defaultRange: '1d' },
  // No 1d tab for these two: neither has a time-of-day series, only one value per day.
  training_load: { key: 'training_load', title: 'Training load', ranges: ['4w', '12w', '6m'], defaultRange: '4w' },
  vo2_max: { key: 'vo2_max', title: 'VO2 max', ranges: ['4w', '12w', '6m'], defaultRange: '4w' },
  respiration: { key: 'respiration', title: 'Breathing', ranges: ['1d', '7d', '4w'], defaultRange: '1d' },
  spo2: { key: 'spo2', title: 'Blood oxygen', ranges: ['1d', '7d', '4w'], defaultRange: '1d' },
};

export function isMetricKey(s: string | undefined): s is MetricKey {
  return s !== undefined && Object.prototype.hasOwnProperty.call(METRICS, s);
}

/** Monday on or before `day`. */
export function weekStart(day: string): string {
  const dow = new Date(`${day}T00:00:00Z`).getUTCDay(); // 0 is Sunday
  return addDays(day, -((dow + 6) % 7));
}

export interface MetricWindow {
  from: string;
  to: string;
  /** The day the period is anchored on: `end` clamped to today. */
  anchor: string;
  /** The period reaches today, so there is no later one. */
  atToday: boolean;
}

/**
 * The window of a tab anchored on `end` (null or a bad value: today; a later day: today). Rolling
 * windows end on the anchor. An intensity minutes week is the calendar week (Monday to Sunday) that
 * holds the anchor, as the goal is a weekly one; its last days may lie after today.
 */
export function metricWindow(metric: MetricKey, range: RangeKey, end: string | null, today: string): MetricWindow {
  const anchor = isIsoDay(end ?? undefined) && (end as string) < today ? (end as string) : today;
  if (metric === 'intensity_minutes' && range === '7d') {
    const from = weekStart(anchor);
    const to = addDays(from, 6);
    return { from, to, anchor, atToday: to >= today };
  }
  return { from: addDays(anchor, -(RANGE_DAYS[range] - 1)), to: anchor, anchor, atToday: anchor >= today };
}

/** The anchor of the period before (-1) or after (+1) the one anchored on `anchor`; never after today. */
export function stepAnchor(range: RangeKey, anchor: string, direction: -1 | 1, today: string): string {
  const next = addDays(anchor, direction * RANGE_DAYS[range]);
  return next > today ? today : next;
}

/** The address of a metric page: `/metrics/steps?range=7d&end=2026-11-14` (no `end` for the period that ends today). */
export function metricHref(metric: MetricKey, range: RangeKey, end: string | null, today: string): string {
  const q = new URLSearchParams({ range });
  if (end !== null && end < today) q.set('end', end);
  return `/metrics/${metric}?${q.toString()}`;
}

export interface RangeTarget {
  range: RangeKey;
  end: string;
}

/**
 * Where a tapped row leads. A day row opens that day on 1d; a week row (a bucket of 12w, 6m or 1y)
 * opens the week on 7d, anchored on its last day (the Sunday of an intensity minutes week, which
 * is the calendar week). `null` when the page has no such tab or the row is not a drill-down.
 */
export function drillTarget(def: MetricDef, bucket: 'day' | 'week', start: string, today: string): RangeTarget | null {
  const range: RangeKey = bucket === 'day' ? '1d' : '7d';
  if (!def.ranges.includes(range)) return null;
  const end = bucket === 'day' ? start : addDays(start, 6);
  return { range, end: end > today ? today : end };
}
