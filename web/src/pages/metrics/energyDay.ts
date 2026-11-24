import type { MetricRange } from '../../api/generated/MetricRange';
import type { TimedValue } from '../../api/generated/TimedValue';
import { fmtInt } from '../../lib/format';
import { signed } from '../today/format';
import { bucketAxis, bucketStarts, values } from './model';

// The Energy page's day: sums over the energy line, and the one sentence under the timeline. The
// rules are plain and written here, not guessed by a model: how much sleep recharged, what drained
// most (activities against everything else), where the day stands.

export interface EnergyPoint {
  /** Milliseconds since the epoch. */
  t: number;
  v: number;
}

/** The series as sorted points; samples without a value or a valid time are dropped. */
export function energyPoints(series: readonly TimedValue[]): EnergyPoint[] {
  return series
    .map((p) => ({ t: Date.parse(p.time), v: p.value }))
    .filter((p): p is EnergyPoint => !Number.isNaN(p.t) && p.v !== null)
    .sort((a, b) => a.t - b.t);
}

/** Sum of the rises between neighbouring samples; a rise belongs to the window its later sample is in (`from` excluded, `to` included). */
export function risesIn(points: readonly EnergyPoint[], from: number, to: number): number {
  let sum = 0;
  for (let i = 1; i < points.length; i += 1) {
    const b = points[i]!;
    const d = b.v - points[i - 1]!.v;
    if (d > 0 && b.t > from && b.t <= to) sum += d;
  }
  return sum;
}

/** Sum of the falls (a positive number) between neighbouring samples, windowed like `risesIn`. */
export function fallsIn(points: readonly EnergyPoint[], from: number, to: number): number {
  let sum = 0;
  for (let i = 1; i < points.length; i += 1) {
    const b = points[i]!;
    const d = points[i - 1]!.v - b.v;
    if (d > 0 && b.t > from && b.t <= to) sum += d;
  }
  return sum;
}

export interface DayActivity {
  /** `run`, `strength`, ... as the activity list names it. */
  name: string;
  start: number;
  end: number;
}

export interface DayFacts {
  points: readonly EnergyPoint[];
  sleep: { start: number; end: number } | null;
  activities: readonly DayActivity[];
  /** Mean stress of the day, 0 to 100. */
  stressAverage: number | null;
  /** The day is today: its last value is "now". */
  isToday: boolean;
  /** The latest value the server reports for "now" (today). */
  now: number | null;
}

/** Stress at or above this mean (the top of "low", 26 to 50) is named as a reason for a drain. */
export const STRESS_NOTICEABLE = 40;

/**
 * One plain sentence about the day, or `null` without energy samples:
 * 1. sleep: how much of the rise came while asleep (when a night is in the series);
 * 2. the drain: activities against the rest, whichever took at least half of it;
 * 3. where the day stands: now for today, the last value for a day that is over.
 */
export function daySentence(f: DayFacts): string | null {
  const pts = f.points;
  if (pts.length < 2) return null;
  const parts: string[] = [];

  if (f.sleep) {
    const recharge = Math.round(risesIn(pts, f.sleep.start, f.sleep.end));
    parts.push(recharge >= 1 ? `Sleep recharged you by ${recharge}.` : 'Sleep added little energy.');
  }

  const total = fallsIn(pts, -Infinity, Infinity);
  if (total >= 1) {
    const inActivities = f.activities.reduce((s, a) => s + fallsIn(pts, a.start, a.end), 0);
    const sum = Math.round(total);
    const act = Math.round(Math.min(inActivities, total));
    const rest = sum - act;
    if (f.activities.length > 0 && act * 2 >= sum) {
      const names = [...new Set(f.activities.map((a) => a.name.toLowerCase()))];
      const what = names.length === 1 ? `Your ${names[0]}` : 'Your activities';
      parts.push(`${what} drained most, ${signed(-act)} of ${signed(-sum)}.`);
    } else if (f.stressAverage !== null && f.stressAverage >= STRESS_NOTICEABLE) {
      parts.push(`Stress and daily movement drained most, ${signed(-rest)} of ${signed(-sum)}.`);
    } else {
      parts.push(`Everyday movement drained most, ${signed(-rest)} of ${signed(-sum)}.`);
    }
  } else {
    parts.push('Little drained your energy.');
  }

  const last = pts[pts.length - 1]!.v;
  const end = f.isToday ? (f.now ?? last) : last;
  parts.push(f.isToday ? `It is at ${fmtInt(end)} now.` : `The day ended at ${fmtInt(end)}.`);
  return parts.join(' ');
}

// ---- 7 days and 4 weeks: charged and drained per day ----------------------------------------------------

export interface FlowChart {
  labels: string[];
  axisLabels?: string[];
  charged: (number | null)[];
  /** Drained as positive numbers: the chart draws them downward. */
  drained: (number | null)[];
  summary: string;
  hasData: boolean;
}

/** Charged (up) and drained (down) for every bucket of an energy range. */
export function flowChart(d: MetricRange, dayLabel: (iso: string) => string): FlowChart {
  const starts = bucketStarts(d);
  const charged = values(d, 'charged');
  const drained = values(d, 'drained');
  const few = d.bucket === 'day' && starts.length <= 7;
  const hasData = charged.some((v) => v !== null) || drained.some((v) => v !== null);
  const rows = starts
    .map((s, i) => (charged[i] == null && drained[i] == null ? null : `${dayLabel(s)} +${fmtInt(charged[i])} charged, ${fmtInt(drained[i])} drained`))
    .filter((r): r is string => r !== null);
  return {
    labels: few ? starts.map(dayLabel) : starts.map(() => ''),
    ...(few ? {} : { axisLabels: bucketAxis(d).map((l) => l.text) }),
    charged,
    drained,
    summary: hasData ? `Energy charged and drained ${d.bucket === 'week' ? 'per week' : 'per day'}: ${rows.join('; ')}.` : 'No energy readings in this period.',
    hasData,
  };
}
