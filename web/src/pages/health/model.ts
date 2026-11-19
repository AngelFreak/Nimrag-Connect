// Pure mapping from API responses to what the charts and cards show. Missing values stay null
// (rendered as "unavailable"), never 0.

import type { DayResponse } from '../../api/generated/DayResponse';
import type { NightSummary } from '../../api/generated/NightSummary';
import type { SleepNight } from '../../api/generated/SleepNight';
import type { StressSample } from '../../api/generated/StressSample';
import type { TimedValue } from '../../api/generated/TimedValue';
import type { TrendSeries } from '../../api/generated/TrendSeries';
import type { TrendsResponse } from '../../api/generated/TrendsResponse';
import type { StageSpan, StressBucket } from '../../components';
import { addDays, fmtDay } from '../../lib/dates';
import { fmtDurationMin, fmtFixed, fmtInt, isMissing, type Maybe } from '../../lib/format';
import { clockAt, dayStartMs, daysBetween, epochMs, offsetMin } from './time';

export const secToMin = (s: Maybe<number>): number | null => (isMissing(s) ? null : s / 60);

/** Seconds as "7h 12m"; null when missing. */
export const fmtSeconds = (s: Maybe<number>): string | null => fmtDurationMin(secToMin(s));

const mean = (v: number[]): number | null => (v.length ? v.reduce((a, b) => a + b, 0) / v.length : null);
const present = (v: (number | null | undefined)[]): number[] => v.filter((x): x is number => !isMissing(x));

// ---- sleep ----------------------------------------------------------------------------------

export function scoreLabel(label: string | null | undefined): string | null {
  return label ? label.charAt(0).toUpperCase() + label.slice(1) : null;
}

/** Sleep-stage spans in minutes from bedtime, for the timeline. Stages the watch could not measure are left out. */
export function stageSpans(night: SleepNight): { spans: StageSpan[]; totalMinutes: number } {
  const start = epochMs(night.start);
  const totalMinutes = (epochMs(night.end) - start) / 60_000;
  const spans: StageSpan[] = [];
  for (const seg of night.stages) {
    if (seg.stage === 'unmeasurable') continue;
    const from = Math.max(0, (epochMs(seg.start) - start) / 60_000);
    const to = Math.min(totalMinutes, (epochMs(seg.end) - start) / 60_000);
    if (to > from) spans.push({ stage: seg.stage, from, to });
  }
  return { spans, totalMinutes };
}

/** Five evenly spaced clock labels from bedtime to wake-up. */
export function sleepTicks(night: SleepNight): string[] {
  const start = epochMs(night.start);
  const span = epochMs(night.end) - start;
  const off = offsetMin(night.start);
  return [0, 0.25, 0.5, 0.75, 1].map((f) => clockAt(start + span * f, off));
}

export function stageSummary(night: SleepNight): string {
  const { spans } = stageSpans(night);
  if (!night.stages_available || spans.length === 0) return 'Sleep stages were not recorded for this night.';
  const parts: string[] = [];
  const add = (name: string, s: number | null) => {
    const t = fmtSeconds(s);
    if (t) parts.push(`${name} ${t}`);
  };
  add('deep sleep', night.deep_s);
  add('light sleep', night.light_s);
  add('REM', night.rem_s);
  add('awake', night.awake_s);
  const n = night.awakenings;
  const awakenings = n === null ? '' : `, ${n} awakening${n === 1 ? '' : 's'}`;
  return `Sleep stages through the night: ${parts.join(', ')}${awakenings}.`;
}

export interface NightBars {
  dates: string[];
  /** Minutes asleep per calendar day, null on nights without data. */
  values: (number | null)[];
  labels: string[];
}

/** One slot per calendar day ending at the latest night, so a missing night is a visible gap. */
export function nightBars(nights: NightSummary[], count: number): NightBars {
  const last = nights.at(-1)?.date;
  if (!last) return { dates: [], values: [], labels: [] };
  const byDate = new Map(nights.map((n) => [n.date, n] as const));
  const dates = Array.from({ length: count }, (_, i) => addDays(last, i - (count - 1)));
  return {
    dates,
    values: dates.map((d) => secToMin(byDate.get(d)?.duration_s ?? null)),
    labels: dates.map((d) => fmtDay(d, { weekday: 'short' })),
  };
}

/** A handful of evenly spaced "5 Nov" labels for a longer axis. */
export function axisDates(dates: string[], n = 4): string[] {
  if (dates.length === 0) return [];
  const idx = Array.from({ length: n }, (_, i) => Math.round((i * (dates.length - 1)) / (n - 1)));
  return idx.map((i) => fmtDay(dates[i] as string, { day: 'numeric', month: 'short' }));
}

export function nightsSummary(nights: NightSummary[], count: number, avgScore: number | null): string | null {
  const mins = present(nights.map((n) => secToMin(n.duration_s)));
  if (mins.length === 0) return null;
  const avg = fmtDurationMin(mean(mins));
  const lo = fmtDurationMin(Math.min(...mins));
  const hi = fmtDurationMin(Math.max(...mins));
  const score = avgScore === null ? '' : `, average score ${Math.round(avgScore)}`;
  return `Sleep averaged ${avg} over ${mins.length} of the last ${count} nights, between ${lo} and ${hi}${score}.`;
}

// ---- day ------------------------------------------------------------------------------------

export interface HourSeries {
  /** Hours since local midnight, with null anchors at 0 and 24 so the axis spans the whole day. */
  x: number[];
  y: (number | null)[];
}

export function hourSeries(date: string, series: TimedValue[]): HourSeries | null {
  const first = series[0];
  if (!first) return null;
  const start = dayStartMs(date, first.time);
  const x = [0];
  const y: (number | null)[] = [null];
  for (const p of series) {
    const h = (epochMs(p.time) - start) / 3_600_000;
    if (h <= 0 || h >= 24) continue;
    x.push(h);
    y.push(p.value);
  }
  x.push(24);
  y.push(null);
  return present(y).length === 0 ? null : { x, y };
}

const BUCKET_MIN = 30;

/** Stress samples folded into 30-minute columns across the day (48 slots; empty slots stay empty). */
export function stressBuckets(date: string, samples: StressSample[]): StressBucket[] {
  const first = samples[0];
  if (!first) return [];
  const start = dayStartMs(date, first.time);
  const slots = (24 * 60) / BUCKET_MIN;
  const groups: StressSample[][] = Array.from({ length: slots }, () => []);
  for (const s of samples) {
    const i = Math.floor((epochMs(s.time) - start) / (BUCKET_MIN * 60_000));
    if (i >= 0 && i < slots) groups[i]?.push(s);
  }
  return groups.map((g): StressBucket => {
    if (g.length === 0) return { level: 'low', value: null };
    const count = new Map<StressSample['level'], number>();
    for (const s of g) count.set(s.level, (count.get(s.level) ?? 0) + 1);
    const [level] = [...count.entries()].sort((a, b) => b[1] - a[1])[0] as [StressSample['level'], number];
    if (level === 'rest' || level === 'activity') return { level, value: null };
    return { level, value: mean(present(g.map((s) => s.stress))) };
  });
}

export function hrAside(hr: DayResponse['heart_rate']): string | null {
  const parts: string[] = [];
  if (hr.resting_hr_bpm !== null) parts.push(`Resting ${fmtInt(hr.resting_hr_bpm)}`);
  if (hr.day.high_bpm !== null) parts.push(`High ${fmtInt(hr.day.high_bpm)}`);
  return parts.length ? parts.join(' · ') : null;
}

export function hrSummary(hr: DayResponse['heart_rate']): string {
  const { low_bpm: lo, high_bpm: hi } = hr.day;
  if (lo === null || hi === null) return 'No heart rate readings for this day.';
  const rest = hr.resting_hr_bpm === null ? '' : `resting ${fmtInt(hr.resting_hr_bpm)} bpm, `;
  return `Heart rate across the day: ${rest}ranging from ${fmtInt(lo)} to ${fmtInt(hi)} bpm.`;
}

const LEVEL_WORD: Record<string, string> = { rest: 'resting', low: 'low', medium: 'medium', high: 'high' };

export function stressAside(stress: DayResponse['stress']): string | null {
  if (stress.average === null) return null;
  const main = stress.main_level ? ` · mostly ${LEVEL_WORD[stress.main_level] ?? stress.main_level}` : '';
  return `Average ${fmtInt(stress.average)}${main}`;
}

export function stressSummary(stress: DayResponse['stress']): string {
  if (stress.average === null && stress.series.length === 0) return 'No stress readings for this day.';
  const parts: string[] = [];
  if (stress.average !== null) parts.push(`averaged ${fmtInt(stress.average)}`);
  if (stress.main_level) parts.push(`mostly ${LEVEL_WORD[stress.main_level] ?? stress.main_level}`);
  return `Stress ${parts.join(', ')}.`;
}

export function energyAside(e: DayResponse['energy']): string | null {
  if (e.high === null || e.low === null) return null;
  return `High ${fmtInt(e.high)} · Low ${fmtInt(e.low)}`;
}

export function energySummary(e: DayResponse['energy']): string {
  if (e.high === null || e.low === null) return 'No energy readings for this day.';
  const flow = e.charged === null || e.drained === null ? '' : `, ${fmtInt(e.charged)} charged and ${fmtInt(e.drained)} drained`;
  return `Energy across the day ranged from ${fmtInt(e.low)} to ${fmtInt(e.high)}${flow}.`;
}

export function stepsPercent(steps: number | null, goal: number): number | null {
  return steps === null || goal <= 0 ? null : Math.round((steps / goal) * 100);
}

const KIND_NAME: Record<string, string> = { run: 'Run', walk: 'Walk', hike: 'Hike', cycle: 'Ride', swim: 'Swim', strength: 'Strength', training: 'Training', other: 'Activity' };
export const kindName = (kind: string): string => KIND_NAME[kind] ?? 'Activity';

// ---- trends ---------------------------------------------------------------------------------

export const RANGES = [
  { value: '7d', label: '7 days' },
  { value: '4w', label: '4 weeks' },
  { value: '3m', label: '3 months' },
  { value: '1y', label: '1 year' },
] as const;
export type RangeKey = (typeof RANGES)[number]['value'];
export const rangeLabel = (r: RangeKey): string => RANGES.find((x) => x.value === r)?.label ?? r;

export interface TrendXY {
  /** Days since the start of the range, with null anchors at both ends so the axis spans the range. */
  x: number[];
  y: (number | null)[];
}

export function trendXY(series: TrendSeries, from: string, to: string, scale = 1): TrendXY | null {
  const pts = series.points.filter((p) => !isMissing(p.value));
  if (pts.length === 0) return null;
  const span = Math.max(1, daysBetween(from, to));
  const x = [0, ...pts.map((p) => daysBetween(from, p.date)), span];
  const y = [null, ...pts.map((p) => (p.value as number) * scale), null];
  return { x, y };
}

/** Four evenly spaced date labels for the bottom of a chart. */
export function rangeAxis(t: Pick<TrendsResponse, 'from' | 'to'>): string[] {
  const span = daysBetween(t.from, t.to);
  const fmt: Intl.DateTimeFormatOptions = span > 200 ? { month: 'short', year: 'numeric' } : { day: 'numeric', month: 'short' };
  return [0, 1, 2, 3].map((i) => fmtDay(addDays(t.from, Math.round((i * span) / 3)), fmt));
}

const num = (v: number | null, digits = 0): string | null => (digits === 0 ? fmtInt(v) : fmtFixed(v, digits));

export function hrvAside(t: TrendsResponse['hrv']): string | null {
  const avg = num(t.series.average);
  if (avg === null) return null;
  const range = t.baseline_low_ms !== null && t.baseline_high_ms !== null ? ` · range ${num(t.baseline_low_ms)} to ${num(t.baseline_high_ms)}` : '';
  return `Avg ${avg} ms${range}`;
}

/** "HRV averaged 61 ms, within your range on 25 of 28 nights". */
export function hrvSummary(t: TrendsResponse['hrv']): string {
  const vals = present(t.series.points.map((p) => p.value));
  const avg = num(t.series.average);
  if (vals.length === 0 || avg === null) return 'No overnight HRV readings in this range.';
  const { baseline_low_ms: lo, baseline_high_ms: hi } = t;
  if (lo === null || hi === null) {
    return `HRV averaged ${avg} ms over ${vals.length} night${vals.length === 1 ? '' : 's'}; the watch has not set a personal range yet.`;
  }
  const inside = vals.filter((v) => v >= lo && v <= hi).length;
  return `HRV averaged ${avg} ms, within your range on ${inside} of ${vals.length} nights.`;
}

function plainSummary(name: string, s: TrendSeries, unit: string, noun: string, digits = 0): string {
  const n = present(s.points.map((p) => p.value)).length;
  const avg = num(s.average, digits);
  if (n === 0 || avg === null) return `No ${name.toLowerCase()} readings in this range.`;
  const u = unit ? ` ${unit}` : '';
  const spread = s.min === null || s.max === null ? '' : `, between ${num(s.min, digits)} and ${num(s.max, digits)}${u}`;
  return `${name} averaged ${avg}${u}${spread}, over ${n} ${noun}${n === 1 ? '' : 's'}.`;
}

export const rhrSummary = (s: TrendSeries): string => plainSummary('Resting heart rate', s, 'bpm', 'day');
export const breathingSummary = (s: TrendSeries): string => plainSummary('Breathing rate', s, 'breaths per minute', 'night', 1);
export const spo2Summary = (s: TrendSeries): string => plainSummary('Overnight blood oxygen', s, '%', 'night', 1);
export const scoreSummary = (s: TrendSeries): string => plainSummary('Sleep score', s, '', 'night');

export function durationSummary(s: TrendSeries): string {
  const mins = present(s.points.map((p) => secToMin(p.value)));
  if (mins.length === 0) return 'No sleep duration readings in this range.';
  return `Sleep averaged ${fmtDurationMin(mean(mins))}, between ${fmtDurationMin(Math.min(...mins))} and ${fmtDurationMin(Math.max(...mins))}, over ${mins.length} night${mins.length === 1 ? '' : 's'}.`;
}

export function rangeAside(s: TrendSeries, unit: string, digits = 0): string | null {
  const avg = num(s.average, digits);
  if (avg === null) return null;
  const u = unit ? ` ${unit}` : '';
  const spread = s.min !== null && s.max !== null ? ` · ${num(s.min, digits)} to ${num(s.max, digits)}` : '';
  return `Avg ${avg}${u}${spread}`;
}
