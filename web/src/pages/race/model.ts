import type { RaceHistoryPoint } from '../../api/generated/RaceHistoryPoint';
import type { RacePrediction } from '../../api/generated/RacePrediction';
import { addDays, fmtDay } from '../../lib/dates';
import type { XLabel } from '../../components/SeriesChart';

// Words and small helpers of the race predictor page. The numbers come from the server
// (GET /api/race-predictions); nothing here estimates a time.

export const ESTIMATE_LABEL = 'Nimrag estimate — not Garmin’s Race Predictor';

/** The distances in the order the page lists them (metres, as the server sends them). */
export const DISTANCES = [5000, 10000, 21097.5, 42195] as const;
export type Distance = (typeof DISTANCES)[number];

export const DISTANCE_NAME: Record<Distance, string> = {
  5000: '5 km',
  10000: '10 km',
  21097.5: 'Half marathon',
  42195: 'Marathon',
};

/** The short names on the chips above the chart. */
export const DISTANCE_CHIP: Record<Distance, string> = {
  5000: '5 km',
  10000: '10 km',
  21097.5: 'Half',
  42195: 'Marathon',
};

export function distanceName(metres: number): string {
  return (DISTANCE_NAME as Record<number, string>)[metres] ?? `${metres / 1000} km`;
}

const MINUS = '−';

const pad = (n: number) => String(n).padStart(2, '0');

/** Whole seconds as "m:ss" under an hour, "h:mm:ss" from an hour on. */
export function fmtRaceTime(seconds: number): string {
  const total = Math.round(Math.abs(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** "4:12 /km" from seconds per km, rounded to the whole second. */
export function fmtPace(secondsPerKm: number): string {
  return `${fmtRaceTime(secondsPerKm)} /km`;
}

/** The change from 4 weeks ago: "−0:42 vs 4 weeks ago" (faster), "+1:05 vs 4 weeks ago", or "No change". */
export function fmtChange(seconds: number | null): { text: string; tone: 'faster' | 'slower' | 'same' } | null {
  if (seconds === null) return null;
  const whole = Math.round(seconds);
  if (whole === 0) return { text: 'No change in 4 weeks', tone: 'same' };
  const sign = whole < 0 ? MINUS : '+';
  return { text: `${sign}${fmtRaceTime(whole)} vs 4 weeks ago`, tone: whole < 0 ? 'faster' : 'slower' };
}

/** What the estimate is based on, in one sentence. */
export function basisText(basis: 'efforts' | 'vo2max' | 'both' | null, vo2max: number | null): string {
  const vo2 = vo2max === null ? 'your VO2 max' : `your VO2 max of ${vo2max.toFixed(1)}`;
  switch (basis) {
    case 'efforts':
      return 'Based on your best efforts in the last 6 weeks.';
    case 'vo2max':
      return `Based on ${vo2}. No run with GPS in the last 6 weeks.`;
    case 'both':
      return `Based on your best efforts in the last 6 weeks and ${vo2}.`;
    default:
      return '';
  }
}

export const HISTORY_OPTIONS = [
  { value: '4w', label: '4 weeks' },
  { value: '6m', label: '6 months' },
] as const;
export type HistoryKey = (typeof HISTORY_OPTIONS)[number]['value'];

/** The history points a period shows: the last 4 weekly points, or all of them (6 months). */
export function pointsFor(history: RaceHistoryPoint[], key: HistoryKey): RaceHistoryPoint[] {
  return key === '4w' ? history.slice(-4) : history;
}

function daysBetween(from: string, to: string): number {
  return Math.round((Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / 86_400_000);
}

function findAt(p: RaceHistoryPoint, metres: number): RacePrediction | undefined {
  return p.predictions.find((x) => x.distance_m === metres);
}

export interface RaceChart {
  x: number[];
  /** Minutes (the axis writes them as m:ss). */
  y: (number | null)[];
  xLabels: XLabel[];
  summary: string;
  count: number;
}

/** The chart of one distance: x counts days since the first point, y is the time in minutes. */
export function raceChart(points: RaceHistoryPoint[], metres: number): RaceChart {
  const rows = points.flatMap((p) => {
    const hit = findAt(p, metres);
    return hit ? [{ date: p.date, seconds: hit.seconds }] : [];
  });
  const name = distanceName(metres);
  if (rows.length === 0) return { x: [], y: [], xLabels: [], summary: `No ${name} estimate in this period.`, count: 0 };
  const first = rows[0]!;
  const last = rows[rows.length - 1]!;
  const span = Math.max(1, daysBetween(first.date, last.date));
  const x = rows.map((r) => daysBetween(first.date, r.date));
  const y = rows.map((r) => r.seconds / 60);
  const picks = span > 14 ? [0, 1, 2, 3] : [0, 1];
  const xLabels = picks.map((i) => {
    const day = Math.round((i * span) / (picks.length - 1));
    return { value: day, text: fmtDay(addDays(first.date, day), { day: 'numeric', month: 'short' }) };
  });
  const fastest = rows.reduce((a, b) => (b.seconds < a.seconds ? b : a));
  const summary =
    rows.length === 1
      ? `${name}: ${fmtRaceTime(first.seconds)} on ${fmtDay(first.date, { day: 'numeric', month: 'short' })}.`
      : `${name}: ${fmtRaceTime(first.seconds)} on ${fmtDay(first.date, { day: 'numeric', month: 'short' })}, ${fmtRaceTime(last.seconds)} on ${fmtDay(last.date, { day: 'numeric', month: 'short' })}. Fastest ${fmtRaceTime(fastest.seconds)}.`;
  return { x, y, xLabels, summary, count: rows.length };
}
