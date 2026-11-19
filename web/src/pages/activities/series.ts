// Turns the API's columnar samples into chart inputs and short text summaries.
// Missing samples stay `null` (a gap in the line), never 0.

import type { ActivitySeries } from '../../api/generated/ActivitySeries';
import type { TimedValue } from '../../api/generated/TimedValue';
import { fmtClock, fmtInt, isMissing } from '../../lib/format';

export interface Spread {
  min: number;
  max: number;
  avg: number;
}

/** Min, max and mean of the present values; `null` without any. */
export function spread(values: readonly (number | null | undefined)[]): Spread | null {
  let n = 0;
  let sum = 0;
  let min = Infinity;
  let max = -Infinity;
  for (const v of values) {
    if (isMissing(v)) continue;
    n += 1;
    sum += v;
    if (v < min) min = v;
    if (v > max) max = v;
  }
  return n === 0 ? null : { min, max, avg: sum / n };
}

/** Minutes on the x axis. */
export function minutesAxis(elapsedSeconds: readonly number[]): number[] {
  return elapsedSeconds.map((s) => Math.round((s / 60) * 100) / 100);
}

/** Slower than this (about 33 min/km) counts as standing still: a gap, not a spike. */
const MIN_MOVING_MPS = 0.5;

/** Pace in minutes per km (as a decimal); standing still is a gap. */
export function paceMinutes(speedMps: readonly (number | null)[]): (number | null)[] {
  return speedMps.map((v) => (isMissing(v) || v < MIN_MOVING_MPS ? null : 1000 / v / 60));
}

export function speedKmh(speedMps: readonly (number | null)[]): (number | null)[] {
  return speedMps.map((v) => (isMissing(v) ? null : v * 3.6));
}

/** "5:56" from decimal minutes. */
export function clockFromMinutes(minutes: number): string {
  return fmtClock(minutes * 60) ?? '';
}

export function durationText(elapsedSeconds: readonly number[]): string {
  const last = elapsedSeconds[elapsedSeconds.length - 1];
  if (last === undefined) return '';
  const min = Math.round(last / 60);
  return min < 1 ? 'under a minute' : `${min} minutes`;
}

export function hrSummary(series: ActivitySeries | null): string {
  const s = series ? spread(series.heart_rate_bpm) : null;
  if (!series || !s) return 'No heart rate samples were recorded.';
  return `Heart rate over ${durationText(series.elapsed_s)}: lowest ${fmtInt(s.min)}, average ${fmtInt(s.avg)}, highest ${fmtInt(s.max)} beats per minute.`;
}

export function paceSummary(values: readonly (number | null)[], elapsed: readonly number[]): string {
  const s = spread(values);
  if (!s) return 'No pace samples were recorded.';
  return `Pace over ${durationText(elapsed)}: fastest ${clockFromMinutes(s.min)}, average ${clockFromMinutes(s.avg)}, slowest ${clockFromMinutes(s.max)} minutes per kilometre.`;
}

export function speedSummary(values: readonly (number | null)[], elapsed: readonly number[]): string {
  const s = spread(values);
  if (!s) return 'No speed samples were recorded.';
  return `Speed over ${durationText(elapsed)}: slowest ${s.min.toFixed(1)}, average ${s.avg.toFixed(1)}, fastest ${s.max.toFixed(1)} kilometres per hour.`;
}

export function elevationSummary(values: readonly (number | null)[], elapsed: readonly number[]): string {
  const s = spread(values);
  if (!s) return 'No elevation samples were recorded.';
  return `Elevation over ${durationText(elapsed)}: lowest ${fmtInt(s.min)} m, highest ${fmtInt(s.max)} m.`;
}

/** Heart-rate points of a strength session as minutes since the first point. */
export function timedToMinutes(points: readonly TimedValue[]): { x: number[]; y: (number | null)[] } {
  const first = points[0] ? Date.parse(points[0].time) : NaN;
  if (!Number.isFinite(first)) return { x: [], y: [] };
  return {
    x: points.map((p) => Math.round(((Date.parse(p.time) - first) / 60000) * 100) / 100),
    y: points.map((p) => p.value),
  };
}
