// Colours for the route line: every point of the route gets a colour from a value along the way
// (pace, speed, heart rate or elevation). Cool is slow or low, warm is fast or high.

import { isMissing } from '../../lib/format';
export { colorRuns, type ColorRun } from '../../lib/colorRuns';
import { fmtPace } from './format';

export type RouteMetric = 'pace' | 'speed' | 'hr' | 'elevation';
export type RouteChoice = RouteMetric | 'plain';

export const ROUTE_CHOICE_LABELS: Record<RouteChoice, string> = {
  pace: 'Pace',
  speed: 'Speed',
  hr: 'Heart rate',
  elevation: 'Elevation',
  plain: 'Plain',
};

/** Cool to warm: blue, cyan, green, amber, red. Hex values: the map draws on a canvas, not with CSS. */
export const RAMP_STOPS: readonly (readonly [number, string])[] = [
  [0, '#3a6fe8'],
  [0.25, '#14a7c7'],
  [0.5, '#4cc075'],
  [0.75, '#f2b82e'],
  [1, '#e5484d'],
];

/** The ramp is cut into this many steps, so a long route becomes a few dozen runs of one colour. */
const RAMP_STEPS = 32;
/** The scale ignores the slowest and fastest 5 % of the samples, so one GPS spike does not flatten it. */
const CLIP_LOW = 0.05;
const CLIP_HIGH = 0.95;
/** Slower than this (about 33 min/km) is standing still: no pace there. */
const MIN_MOVING_MPS = 0.5;

function channel(hex: string, i: number): number {
  return parseInt(hex.slice(1 + i * 2, 3 + i * 2), 16);
}

/** The colour of the ramp at `t` (0 cool .. 1 warm), snapped to one of 33 steps. */
export function rampColor(t: number): string {
  const clamped = Math.min(1, Math.max(0, Number.isFinite(t) ? t : 0));
  const snapped = Math.round(clamped * RAMP_STEPS) / RAMP_STEPS;
  let k = 1;
  while (k < RAMP_STOPS.length - 1 && snapped > (RAMP_STOPS[k] as readonly [number, string])[0]) k += 1;
  const [t0, c0] = RAMP_STOPS[k - 1] as readonly [number, string];
  const [t1, c1] = RAMP_STOPS[k] as readonly [number, string];
  const f = (snapped - t0) / (t1 - t0);
  const mix = (i: number) => Math.round(channel(c0, i) + (channel(c1, i) - channel(c0, i)) * f);
  return `#${[0, 1, 2].map((i) => mix(i).toString(16).padStart(2, '0')).join('')}`;
}

/** The ramp as a CSS gradient, for the legend. */
export function rampGradient(): string {
  return `linear-gradient(90deg, ${RAMP_STOPS.map(([t, c]) => `${c} ${t * 100}%`).join(', ')})`;
}

/** The p-quantile (0..1) of a sorted list, interpolated between neighbours. */
export function quantile(sorted: readonly number[], p: number): number {
  if (sorted.length === 0) return NaN;
  const pos = Math.min(1, Math.max(0, p)) * (sorted.length - 1);
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  const a = sorted[lo] as number;
  const b = sorted[hi] as number;
  return a + (b - a) * (pos - lo);
}

/** Mean of the present values within `radius` points either side; a gap stays a gap. */
export function smooth(values: readonly (number | null)[], radius: number): (number | null)[] {
  return values.map((v, i) => {
    if (isMissing(v)) return null;
    let sum = 0;
    let n = 0;
    for (let j = Math.max(0, i - radius); j <= Math.min(values.length - 1, i + radius); j += 1) {
      const w = values[j];
      if (!isMissing(w)) {
        sum += w;
        n += 1;
      }
    }
    return sum / n;
  });
}

interface PointValues {
  speed_mps?: readonly (number | null)[];
  heart_rate_bpm?: readonly (number | null)[];
  altitude_m?: readonly (number | null)[];
}

/** The value to colour by, per point: speed in m/s (also for pace), bpm or metres. Missing stays `null`. */
export function metricValues(metric: RouteMetric, track: PointValues): (number | null)[] {
  if (metric === 'pace' || metric === 'speed') {
    const moving = (track.speed_mps ?? []).map((v) => (isMissing(v) || v < MIN_MOVING_MPS ? null : v));
    return smooth(moving, 3);
  }
  if (metric === 'hr') return smooth(track.heart_rate_bpm ?? [], 2);
  return smooth(track.altitude_m ?? [], 2);
}

export interface RouteScale {
  /** Value at the cool end and at the warm end. */
  lo: number;
  hi: number;
}

/** The 5th to 95th percentile of the present values; `null` when there is no spread to colour. */
export function routeScale(values: readonly (number | null)[]): RouteScale | null {
  const present = values.filter((v): v is number => !isMissing(v)).sort((a, b) => a - b);
  if (present.length < 2) return null;
  const lo = quantile(present, CLIP_LOW);
  const hi = quantile(present, CLIP_HIGH);
  return hi - lo > 1e-9 ? { lo, hi } : null;
}

/**
 * One colour per point. A value outside the scale takes the end colour; a point without a value
 * keeps the colour of the point before it (or of the first one that has a value).
 */
export function routeColors(values: readonly (number | null)[], scale: RouteScale): string[] {
  const span = scale.hi - scale.lo;
  const own = values.map((v) => (isMissing(v) ? null : rampColor((v - scale.lo) / span)));
  const first = own.find((c) => c !== null) ?? rampColor(0);
  let last = first;
  return own.map((c) => {
    if (c !== null) last = c;
    return last;
  });
}

export interface RouteLegendText {
  /** What the colour shows, in words. */
  what: string;
  /** The value at the cool end and at the warm end. */
  low: string;
  high: string;
}

/** The legend's end values: "6:30 /km" (slow) to "4:50 /km" (fast), "112 bpm" to "171 bpm". */
export function routeLegend(metric: RouteMetric, scale: RouteScale): RouteLegendText {
  switch (metric) {
    case 'pace':
      return { what: 'Pace', low: fmtPace(1000 / scale.lo) ?? '', high: fmtPace(1000 / scale.hi) ?? '' };
    case 'speed':
      return { what: 'Speed', low: `${(scale.lo * 3.6).toFixed(1)} km/h`, high: `${(scale.hi * 3.6).toFixed(1)} km/h` };
    case 'hr':
      return { what: 'Heart rate', low: `${Math.round(scale.lo)} bpm`, high: `${Math.round(scale.hi)} bpm` };
    case 'elevation':
      return { what: 'Elevation', low: `${Math.round(scale.lo)} m`, high: `${Math.round(scale.hi)} m` };
  }
}

type SeriesLike = { speed_mps: readonly (number | null)[]; heart_rate_bpm: readonly (number | null)[]; altitude_m: readonly (number | null)[] } | null;

function varies(values: readonly (number | null)[]): boolean {
  let min = Infinity;
  let max = -Infinity;
  for (const v of values) {
    if (isMissing(v)) continue;
    if (v < min) min = v;
    if (v > max) max = v;
  }
  return max > min;
}

/** The ways to colour this activity's route, in the order of the switch; "Plain" is always last. */
export function routeChoices(kind: string, series: SeriesLike): RouteChoice[] {
  const paced = kind === 'run' || kind === 'walk' || kind === 'hike';
  const out: RouteChoice[] = [];
  if (series && varies(series.speed_mps.map((v) => (isMissing(v) || v < MIN_MOVING_MPS ? null : v)))) out.push(paced ? 'pace' : 'speed');
  if (series && varies(series.heart_rate_bpm)) out.push('hr');
  if (series && varies(series.altitude_m)) out.push('elevation');
  return out.length ? [...out, 'plain'] : [];
}

/** What the switch starts on: pace for runs and walks, speed for rides, a plain line for everything else. */
export function defaultRouteChoice(kind: string, choices: readonly RouteChoice[]): RouteChoice {
  const wanted: RouteChoice = kind === 'cycle' ? 'speed' : kind === 'run' || kind === 'walk' || kind === 'hike' ? 'pace' : 'plain';
  return choices.includes(wanted) ? wanted : 'plain';
}
