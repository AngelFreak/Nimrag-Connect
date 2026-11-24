import type { ReadinessDay } from '../../api/generated/ReadinessDay';
import type { ReadinessFactor } from '../../api/generated/ReadinessFactor';
import type { ReadinessFactorKey } from '../../api/generated/ReadinessFactorKey';
import type { ReadinessLevel } from '../../api/generated/ReadinessLevel';
import type { ReadinessRating } from '../../api/generated/ReadinessRating';
import { addDays, fmtDay, fmtMonthDay, fmtPeriod } from '../../lib/dates';
import { fmtInt } from '../../lib/format';

// Words, colours and small helpers of the readiness page. The numbers and the rules come from the
// server (GET /api/readiness); nothing here decides a score or a rating.

export const ESTIMATE_LABEL = 'Nimrag estimate — not Garmin’s Training Readiness';

export const LEVELS: readonly ReadinessLevel[] = ['poor', 'low', 'moderate', 'high', 'prime'];

export const LEVEL_NAME: Record<ReadinessLevel, string> = {
  poor: 'Poor',
  low: 'Low',
  moderate: 'Moderate',
  high: 'High',
  prime: 'Prime',
};

/** The score range of each level, for the colour key. */
export const LEVEL_RANGE: Record<ReadinessLevel, string> = {
  poor: '0–24',
  low: '25–49',
  moderate: '50–74',
  high: '75–94',
  prime: '95–100',
};

/** Worst to best: red, orange, yellow, green, teal (tokens `--rate-1` to `--rate-5`). */
export const LEVEL_COLOR: Record<ReadinessLevel, string> = {
  poor: 'var(--rate-1)',
  low: 'var(--rate-2)',
  moderate: 'var(--rate-3)',
  high: 'var(--rate-4)',
  prime: 'var(--rate-5)',
};

export const RATING_NAME: Record<ReadinessRating, string> = {
  very_poor: 'Very poor',
  poor: 'Poor',
  moderate: 'Moderate',
  good: 'Good',
  very_good: 'Very good',
};

export const RATING_COLOR: Record<ReadinessRating, string> = {
  very_poor: 'var(--rate-1)',
  poor: 'var(--rate-2)',
  moderate: 'var(--rate-3)',
  good: 'var(--rate-4)',
  very_good: 'var(--rate-5)',
};

interface FactorMeta {
  name: string;
  /** Said under the name when the server has no note of its own. */
  sub: string;
  to: string;
}

export const FACTOR_META: Record<ReadinessFactorKey, FactorMeta> = {
  sleep: { name: 'Last night’s sleep', sub: 'Sleep score of the night', to: '/sleep' },
  recovery: { name: 'Recovery time left', sub: 'On the watch’s recovery timer', to: '/training' },
  hrv: { name: 'HRV, 7-day average', sub: 'Against your balanced range', to: '/hrv' },
  load: { name: '7-day training load', sub: 'Against its target range', to: '/metrics/training_load' },
  sleep_history: { name: 'Sleep, last 7 nights', sub: 'Average sleep score', to: '/sleep?range=4w' },
  stress: { name: 'Stress, last 3 days', sub: 'Average all-day stress', to: '/metrics/stress?range=7d' },
};

/** A number as the API has it: whole when whole, else one decimal ("73.9"); never rounded further. */
export function fmtNum(value: number | null | undefined): string | null {
  if (value === null || value === undefined || !Number.isFinite(value)) return null;
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

/** "85", "0 h", "60 ms"; `null` for a missing factor. */
export function factorValue(f: Pick<ReadinessFactor, 'value' | 'unit'>): string | null {
  const n = fmtNum(f.value);
  if (n === null) return null;
  return f.unit === 'h' || f.unit === 'ms' ? `${n} ${f.unit}` : n;
}

/** What a factor row says under its name: the value, then the server's note or the standing text. */
export function factorSub(f: ReadinessFactor): string {
  const meta = FACTOR_META[f.key];
  const value = factorValue(f);
  if (value === null) return 'Not available yet';
  return [value, f.note ?? meta.sub].join(' · ');
}

export type HistoryKey = '7d' | '4w' | '12w';

export const HISTORY_OPTIONS = [
  { value: '7d', label: '7d', days: 7 },
  { value: '4w', label: '4w', days: 28 },
  { value: '12w', label: '12w', days: 84 },
] as const satisfies readonly { value: HistoryKey; label: string; days: number }[];

export const HISTORY_NAME: Record<HistoryKey, string> = { '7d': 'the last 7 days', '4w': 'the last 4 weeks', '12w': 'the last 12 weeks' };

export function historyWindow(key: HistoryKey, today: string): { from: string; to: string } {
  const days = HISTORY_OPTIONS.find((o) => o.value === key)?.days ?? 7;
  return { from: addDays(today, -(days - 1)), to: today };
}

export interface HistoryChart {
  values: (number | null)[];
  colors: (string | undefined)[];
  labels: string[];
  /** Two dates under a long chart; empty for a week (every bar has its weekday). */
  axis: string[];
  scored: number;
  summary: string;
}

/** The bars of the history chart: a score per day, coloured by level; a day without one is a stub. */
export function historyChart(days: readonly ReadinessDay[], average: number | null): HistoryChart {
  const values = days.map((d) => d.score);
  const colors = days.map((d) => (d.level ? LEVEL_COLOR[d.level] : undefined));
  const short = days.length <= 7;
  const labels = days.map((d) => (short ? fmtDay(d.date, { weekday: 'short' }) : ''));
  const first = days[0];
  const last = days[days.length - 1];
  const axis = short || !first || !last ? [] : [fmtMonthDay(first.date), fmtMonthDay(last.date)];
  const known = days.filter((d): d is ReadinessDay & { score: number } => d.score !== null);
  let summary: string;
  if (!first || !last || known.length === 0) {
    summary = 'No readiness estimate in this period.';
  } else {
    const best = known.reduce((a, b) => (b.score > a.score ? b : a));
    const worst = known.reduce((a, b) => (b.score < a.score ? b : a));
    const avg = fmtNum(average);
    summary = `Readiness ${fmtPeriod(first.date, last.date)}: ${known.length} of ${days.length} days with an estimate${avg ? `, average ${avg}` : ''}, highest ${fmtInt(best.score)} on ${fmtMonthDay(best.date)}, lowest ${fmtInt(worst.score)} on ${fmtMonthDay(worst.date)}.`;
  }
  return { values, colors, labels, axis, scored: known.length, summary };
}
