// Value-axis helpers shared by every chart (uPlot and SVG): nice tick positions and the labels
// that go on them. Numbers only on the ticks; the unit is in the card header or legend.

/** What the numbers on an axis are, which decides the step sizes and how a tick is written. */
export type AxisUnit =
  /** Whole numbers: bpm, steps, ms, score. */
  | 'int'
  /** Whole numbers, or one decimal when the step needs it (kg, ml/kg/min, km/h). */
  | 'decimal'
  /** Decimal minutes per km, written m:ss. */
  | 'pace'
  /** Hours, written 6h (6.5h between). */
  | 'hours'
  /** Minutes, written as hours (6h, 6.5h). */
  | 'minutes';

interface TickOptions {
  /** Candidate step sizes. By default 1, 2, 2.5 and 5 times a power of ten. */
  steps?: number[];
  /** Mantissas of the generated steps (default 1, 2, 2.5, 5): fractional axes drop 2.5, which gives 0.25. */
  mantissas?: number[];
  /** Only whole-number steps (and so whole-number ticks). */
  integer?: boolean;
}

const MANTISSAS = [1, 2, 2.5, 5];
const EPS = 1e-9;

function candidateSteps(span: number, opts: TickOptions): number[] {
  const steps =
    opts.steps ??
    (() => {
      const e = Math.floor(Math.log10(span));
      const out: number[] = [];
      for (let exp = e - 2; exp <= e + 1; exp++) for (const m of opts.mantissas ?? MANTISSAS) out.push(m * 10 ** exp);
      return out;
    })();
  return opts.integer ? steps.filter((s) => s >= 1 && Number.isInteger(s)) : steps;
}

function ticksAt(min: number, max: number, step: number): number[] {
  const out: number[] = [];
  const first = Math.ceil(min / step - EPS);
  for (let i = first; i * step <= max + EPS * step; i++) out.push(Number((i * step).toFixed(10)));
  return out;
}

/**
 * Round tick values inside [min, max]: about `count` of them (never fewer than two when the range
 * has two round numbers in it). Picks the step whose tick count is closest to `count`; on a tie
 * the larger step, so fewer and rounder ticks win.
 */
export function niceTicks(min: number, max: number, count = 4, opts: TickOptions = {}): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return [];
  if (min > max) [min, max] = [max, min];
  if (max - min < EPS) return [min];
  let best: number[] = [];
  let bestScore = Infinity;
  let bestStep = 0;
  for (const step of candidateSteps(max - min, opts)) {
    const ticks = ticksAt(min, max, step);
    if (ticks.length < 2) continue;
    // 2.5 steps (25, 50, 75) read well but lose to 2 and 5 when both fit as well.
    const mantissa = step / 10 ** Math.floor(Math.log10(step) + EPS);
    // Too many ticks cost double: crowded labels are worse than sparse ones.
    const miss = ticks.length - count;
    // Three or more where there is room for them (Garmin Connect draws 3 to 5).
    const score = (miss > 0 ? miss * 2 : -miss) + (count >= 3 && ticks.length < 3 ? 1.5 : 0) + (Math.abs(mantissa - 2.5) < 1e-6 ? 0.1 : 0);
    if (score < bestScore - EPS || (Math.abs(score - bestScore) < EPS && step > bestStep)) {
      best = ticks;
      bestScore = score;
      bestStep = step;
    }
  }
  if (best.length > 0) return best;
  // Not even two round values fit (a very narrow range): the ends themselves.
  return [min, max];
}

const UNIT_STEPS: Partial<Record<AxisUnit, TickOptions>> = {
  int: { integer: true },
  decimal: { mantissas: [1, 2, 5] },
  pace: { steps: [0.25, 0.5, 1, 2, 5, 10, 20] },
  hours: { steps: [0.5, 1, 2, 3, 4, 6, 12, 24] },
  minutes: { steps: [30, 60, 120, 180, 240, 360, 720, 1440] },
};

/** Nice ticks for a unit (whole numbers for `int`, quarter minutes for `pace`, whole hours for `hours`). */
export function unitTicks(min: number, max: number, count: number, unit: AxisUnit = 'int'): number[] {
  return niceTicks(min, max, count, UNIT_STEPS[unit]);
}

/** Smallest number of decimals (0 to 2) that shows every tick exactly. */
function decimalsFor(ticks: number[]): number {
  for (let d = 0; d < 2; d++) {
    if (ticks.every((t) => Math.abs(t * 10 ** d - Math.round(t * 10 ** d)) < 1e-6)) return d;
  }
  return 2;
}

const grouped = new Intl.NumberFormat('en-GB', { maximumFractionDigits: 0 });

/** m:ss from decimal minutes (5.5 is 5:30). */
export function fmtPaceTick(minutes: number): string {
  const total = Math.round(minutes * 60);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

/** The axis label of every tick, written the same way (same number of decimals). */
export function formatTicks(ticks: number[], unit: AxisUnit = 'int'): string[] {
  switch (unit) {
    case 'pace':
      return ticks.map(fmtPaceTick);
    case 'hours':
    case 'minutes': {
      const hours = unit === 'minutes' ? ticks.map((t) => t / 60) : ticks;
      const d = decimalsFor(hours);
      return hours.map((h) => `${h.toFixed(d)}h`);
    }
    case 'decimal': {
      const d = decimalsFor(ticks);
      return ticks.map((t) => t.toFixed(d));
    }
    default:
      return ticks.map((t) => grouped.format(Math.round(t)));
  }
}

/**
 * How many tick labels fit along `heightPx` of plot: one per `2.4 × fontPx`, between 2 and 5
 * (Garmin Connect draws 3 to 5). Labels then keep clear air between them, even at 130% text size
 * on a short chart.
 */
export function tickCountFor(heightPx: number, fontPx: number): number {
  const fit = Math.floor(heightPx / (fontPx * 2.4)) + 1;
  return Math.max(2, Math.min(5, fit));
}

/** Only the extremes: the min and max of the data, for a tiny sparkline (one tick if they are equal). */
export function extremeTicks(values: readonly (number | null | undefined)[]): number[] {
  const v = values.filter((x): x is number => typeof x === 'number' && Number.isFinite(x));
  if (v.length === 0) return [];
  const lo = Math.min(...v);
  const hi = Math.max(...v);
  return hi - lo < EPS ? [lo] : [lo, hi];
}

/**
 * Axis for a chart that starts at zero (bars): the nice ticks and the top of the scale, rounded up
 * to the next tick so the tallest bar never overshoots the top label.
 */
export function zeroBasedScale(rawTop: number, count: number, unit: AxisUnit = 'int'): { ticks: number[]; top: number } {
  const ticks = unitTicks(0, rawTop, count, unit);
  const last = ticks[ticks.length - 1] ?? 0;
  const prev = ticks[ticks.length - 2];
  if (prev === undefined || last >= rawTop - EPS) return { ticks, top: Math.max(last, rawTop) };
  const top = last + (last - prev);
  return { ticks: [...ticks, top], top };
}

/** A value rounded to what its label can show (whole numbers, one decimal, a second of pace). */
export function roundTick(v: number, unit: AxisUnit = 'int'): number {
  switch (unit) {
    case 'int':
    case 'minutes':
      return Math.round(v);
    case 'pace':
      return Math.round(v * 60) / 60;
    default:
      return Math.round(v * 10) / 10;
  }
}
