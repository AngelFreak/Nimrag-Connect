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

export interface TimeTick {
  /** Minutes since the start. */
  minutes: number;
  label: string;
}

const TICK_STEPS_MIN = [1, 2, 5, 10, 15, 20, 30, 60, 120, 180, 240, 360];

/**
 * Evenly spaced time-axis ticks from 0 up to `totalMinutes` (at most `maxTicks`, 0 included),
 * with a step that reads well: 0, 10, 20, 30 min or 0, 1, 2, 3 h. The unit goes on the last label.
 */
export function timeTicks(totalMinutes: number, maxTicks = 5): TimeTick[] {
  if (!Number.isFinite(totalMinutes) || totalMinutes <= 0) return [];
  const step = TICK_STEPS_MIN.find((st) => Math.floor(totalMinutes / st) + 1 <= maxTicks) ?? (TICK_STEPS_MIN[TICK_STEPS_MIN.length - 1] as number);
  const hours = step >= 60;
  const out: TimeTick[] = [];
  for (let m = 0; m <= totalMinutes + 1e-9; m += step) {
    out.push({ minutes: m, label: String(hours ? m / 60 : m) });
  }
  const last = out[out.length - 1];
  if (last) last.label += hours ? ' h' : ' min';
  return out;
}

/** Time labels under an activity chart (0, 10, 20, 30 min), at their minute on the chart's own x axis. */
export function timeLabels(totalMinutes: number): { value: number; text: string }[] {
  const ticks = timeTicks(totalMinutes);
  return ticks.length < 2 ? [] : ticks.map((t) => ({ value: t.minutes, text: t.label }));
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

// ---- desktop activity charts: distance axis, cadence ----------------------------------------------

/** Kilometres on the x axis from the cumulative distance samples; gaps carry the last value forward. `null` without a distance. */
export function distanceAxis(distanceM: readonly (number | null)[]): number[] | null {
  let last = 0;
  let any = false;
  const out = distanceM.map((d) => {
    if (!isMissing(d)) {
      any = true;
      last = Math.max(last, d);
    }
    return Math.round((last / 1000) * 1000) / 1000;
  });
  return any && last > 0 ? out : null;
}

/** Distance-axis labels (0, 1, 2 km): a step that reads well, at most six labels, the unit on the last one. */
export function distanceLabels(totalKm: number): { value: number; text: string }[] {
  if (!Number.isFinite(totalKm) || totalKm <= 0) return [];
  const steps = [0.25, 0.5, 1, 2, 5, 10, 20, 25, 50, 100];
  const step = steps.find((st) => Math.floor(totalKm / st) + 1 <= 6) ?? 100;
  const out: { value: number; text: string }[] = [];
  for (let k = 0; k <= totalKm + 1e-9; k += step) out.push({ value: Math.round(k * 1000) / 1000, text: String(Math.round(k * 100) / 100) });
  const last = out[out.length - 1];
  if (last) last.text += ' km';
  return out.length < 2 ? [] : out;
}

export function cadenceSummary(values: readonly (number | null)[], elapsed: readonly number[], perMinute = 'revolutions a minute'): string {
  const s = spread(values);
  if (!s) return 'No cadence samples were recorded.';
  return `Cadence over ${durationText(elapsed)}: lowest ${fmtInt(s.min)}, average ${fmtInt(s.avg)}, highest ${fmtInt(s.max)} ${perMinute}.`;
}

// ---- the value under the pointer ---------------------------------------------------------------

/** Every value of one sample of an activity, written as shown: "12:30", "5:12 /km", "148 bpm". A missing value is `null`. */
export interface SampleText {
  time: string | null;
  distance: string | null;
  pace: string | null;
  speed: string | null;
  hr: string | null;
  elevation: string | null;
  cadence: string | null;
}

/** The sample at `index`, or `null` when there is none. */
export function sampleAt(series: ActivitySeries | null, index: number | null, cadenceUnit = 'rpm'): SampleText | null {
  if (!series || index === null || index < 0 || index >= series.elapsed_s.length) return null;
  const num = (list: readonly (number | null)[]): number | null => {
    const v = list[index];
    return isMissing(v) ? null : v;
  };
  const speed = num(series.speed_mps);
  const moving = speed !== null && speed >= MIN_MOVING_MPS;
  const dist = num(series.distance_m);
  const hr = num(series.heart_rate_bpm);
  const alt = num(series.altitude_m);
  const cad = num(series.cadence);
  return {
    time: fmtClock(series.elapsed_s[index] ?? null),
    distance: dist === null ? null : `${(dist / 1000).toFixed(2)} km`,
    pace: moving ? `${clockFromMinutes(1000 / speed / 60)} /km` : null,
    speed: speed === null ? null : `${(speed * 3.6).toFixed(1)} km/h`,
    hr: hr === null ? null : `${fmtInt(hr)} bpm`,
    elevation: alt === null ? null : `${fmtInt(alt)} m`,
    cadence: cad === null ? null : `${fmtInt(cad)} ${cadenceUnit}`,
  };
}
