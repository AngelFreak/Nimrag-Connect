// What the metric pages draw from a GET /api/metrics response: chart data, the figures of the
// period, the rows of the list. Pure mapping; a bucket without data stays null (shown as
// "unavailable"), never 0.

import type { MetricRange } from '../../api/generated/MetricRange';
import type { MetricStat } from '../../api/generated/MetricStat';
import type { BandSeries, ChartSeries, LegendItem, StackPart, XLabel } from '../../components';
import { addDays, fmtDay, fmtMonthDay, fmtMonthShort, fmtPeriod, fmtRunDates } from '../../lib/dates';
import { fmtFixed, fmtInt, isMissing, withUnit } from '../../lib/format';
import { LEVEL_COLOR, LEVEL_NAME, LEVEL_RANGE, MEASURED_LEVELS } from '../../lib/stress';
import type { AxisUnit } from '../../lib/ticks';
import { drillTarget, type MetricDef, type MetricKey, type RangeTarget } from './period';

type Values = (number | null)[];

export const values = (d: MetricRange, key: string): Values => d.series.find((s) => s.key === key)?.points.map((p) => p.value) ?? [];

/** The first day of every bucket. */
export const bucketStarts = (d: MetricRange): string[] => d.series[0]?.points.map((p) => p.date) ?? [];

const present = (v: Values): number[] => v.filter((x): x is number => x !== null);

/** The last day a bucket covers: its own for a day bucket, six days on for a week, never after the period. */
export function bucketEnd(d: Pick<MetricRange, 'bucket' | 'to'>, start: string): string {
  if (d.bucket === 'day') return start;
  const end = addDays(start, 6);
  return end > d.to ? d.to : end;
}

/** `n` evenly spaced indexes of a list of `length` (the first and the last included). */
function spread(length: number, n: number): number[] {
  if (length <= n) return Array.from({ length }, (_, i) => i);
  return Array.from({ length: n }, (_, i) => Math.round((i * (length - 1)) / (n - 1)));
}

/** Four dates under the chart, each at its bucket: "Nov 13" over days and weeks, "Dec" over a year. */
export function bucketAxis(d: MetricRange): XLabel[] {
  const starts = bucketStarts(d);
  const text = (day: string) => (d.range === '1y' ? fmtMonthShort(day) : fmtMonthDay(day));
  return spread(starts.length, 4).map((i) => ({ value: i, text: text(starts[i] as string) }));
}

/** Moderate + 2 x vigorous, the figure the weekly goal is measured in. */
export function credited(moderate: number | null, vigorous: number | null): number | null {
  return moderate === null && vigorous === null ? null : (moderate ?? 0) + 2 * (vigorous ?? 0);
}

export function creditedMinutes(d: MetricRange): Values {
  const m = values(d, 'moderate');
  const v = values(d, 'vigorous');
  return m.map((x, i) => credited(x, v[i] ?? null));
}

/**
 * Running total of daily values from the first day; a day without data adds nothing, days after today
 * have none. A period with no value at all has no total, not a flat 0.
 */
export function runningTotal(daily: Values, starts: string[], today: string): Values {
  if (present(daily).length === 0) return daily.map(() => null);
  let sum = 0;
  return daily.map((v, i) => {
    if ((starts[i] as string) > today) return null;
    sum += v ?? 0;
    return sum;
  });
}

// ---- figures ----------------------------------------------------------------------------------------

/** The units that read well after a number in a tile; a longer one goes under it. */
const INLINE_UNITS = new Set(['bpm', 'min', '%', 'brpm']);

/** A figure of the period as a tile shows it: the number with its short unit, and a long unit for the line under it. */
export function statText(s: Pick<MetricStat, 'value' | 'unit'>): { value: string | null; unit: string | null } {
  if (s.value === null) return { value: null, unit: null };
  if (s.unit === 'm') return { value: withUnit(fmtFixed(s.value / 1000, 1), 'km'), unit: null };
  const digits = s.unit === 'brpm' || s.unit === 'ml/kg/min' ? 1 : 0;
  const n = digits === 0 ? fmtInt(s.value) : fmtFixed(s.value, 1);
  if (s.unit === '%') return { value: `${n}%`, unit: null };
  if (INLINE_UNITS.has(s.unit)) return { value: withUnit(n, s.unit), unit: null };
  return { value: n, unit: s.unit === 'steps' || s.unit === '' ? null : s.unit };
}

/** The stats a tile row shows, in order, by key (the response may carry more). */
const TILES: Record<MetricKey, readonly string[]> = {
  heart_rate: ['avg_resting', 'avg_high'],
  stress: ['avg_stress'],
  energy: ['avg_high', 'avg_low', 'avg_charged', 'avg_drained'],
  steps: ['total_steps', 'avg_steps', 'total_distance'],
  intensity_minutes: ['total', 'moderate', 'vigorous', 'avg'],
  training_load: ['avg_acute', 'avg_chronic'],
  vo2_max: ['latest', 'avg'],
  respiration: ['avg_asleep', 'avg_awake'],
  spo2: ['avg', 'lowest'],
};

export function tileStats(metric: MetricKey, d: MetricRange): MetricStat[] {
  return TILES[metric].flatMap((k) => d.stats.find((s) => s.key === k) ?? []);
}

/** How many buckets hold a value in any series. */
export function bucketsWithData(d: MetricRange): number {
  const n = d.series[0]?.points.length ?? 0;
  return Array.from({ length: n }, (_, i) => d.series.some((s) => s.points[i]?.value !== null && s.points[i]?.value !== undefined)).filter(Boolean).length;
}

/** The period in words, for screen readers and the text under the chart. */
export function rangeSummary(def: MetricDef, d: MetricRange): string {
  const period = fmtPeriod(d.from, d.to);
  const n = bucketsWithData(d);
  const total = d.series[0]?.points.length ?? 0;
  if (n === 0) return `Shows after your watch syncs ${def.title.toLowerCase()} (${period}).`;
  const unit = d.bucket === 'day' ? 'day' : 'week';
  const figures = tileStats(def.key, d)
    .map((s) => {
      const t = statText(s);
      return t.value === null ? null : `${s.label.toLowerCase()} ${t.value}${t.unit ? ` ${t.unit}` : ''}`;
    })
    .filter((x): x is string => x !== null);
  return `${def.title} ${period}: ${n} of ${total} ${unit}s with a reading${figures.length ? `; ${figures.join(', ')}` : ''}.`;
}

// ---- chart ------------------------------------------------------------------------------------------

export interface LineChartSpec {
  kind: 'line';
  x: number[];
  series: ChartSeries[];
  xRange: [number, number];
  xLabels: XLabel[];
  yRange?: [number, number];
  yUnit: AxisUnit;
  band?: [number, number] | BandSeries;
  legend: LegendItem[];
}

export interface BarChartSpec {
  kind: 'bars';
  values: Values;
  labels: string[];
  axisLabels?: string[];
  goal?: number;
  color: string;
  belowGoalColor?: string;
  yUnit: AxisUnit;
  legend: LegendItem[];
}

/** Bars stacked by part (minutes of stress per level), with a line chart of a figure under them. */
export interface StackedChartSpec {
  kind: 'stacked';
  parts: StackPart[];
  labels: string[];
  axisLabels?: string[];
  /** The average stress, drawn as its own line chart under the bars. */
  average: LineChartSpec;
  legend: LegendItem[];
}

export type ChartSpec = LineChartSpec | BarChartSpec | StackedChartSpec;

/** Points get a dot when there are few enough for the dots not to touch. */
const dots = (n: number): boolean => n <= 31;

function line(d: MetricRange, series: ChartSeries[], over: Partial<LineChartSpec> = {}): LineChartSpec {
  const n = d.series[0]?.points.length ?? 0;
  return {
    kind: 'line',
    x: Array.from({ length: n }, (_, i) => i),
    series: series.map((s) => ({ ...s, points: dots(n) })),
    xRange: [-0.5, n - 0.5],
    xLabels: bucketAxis(d),
    yUnit: 'int',
    legend: [],
    ...over,
  };
}

const SWATCH: Record<ChartSeries['color'], string> = {
  link: 'var(--link)',
  linkSoft: 'var(--link-hover)',
  heart: 'var(--heart)',
  sleep: 'var(--sleep)',
  energy: 'var(--energy)',
  steps: 'var(--steps)',
  warn: 'var(--warn)',
};
const key = (s: ChartSeries): LegendItem => ({ key: s.label, label: s.label, color: SWATCH[s.color], shape: 'line' });

/** Bottom of a percent axis: the lowest reading rounded down to 5, at most 90 (the top is 100). */
function percentFloor(v: Values): number {
  const low = Math.min(...present(v));
  return Number.isFinite(low) ? Math.min(90, Math.floor(low / 5) * 5) : 90;
}

/** Bars of a few buckets are labelled by weekday; of many, by a few dates. */
function barLabels(d: MetricRange): Pick<BarChartSpec, 'labels' | 'axisLabels'> {
  const starts = bucketStarts(d);
  if (d.bucket === 'day' && starts.length <= 7) return { labels: starts.map((s) => fmtDay(s, { weekday: 'short' })) };
  return { labels: starts.map(() => ''), axisLabels: bucketAxis(d).map((l) => l.text) };
}

/** The running total of credited minutes over a calendar week, with the weekly goal as a flat line. */
/** The goal line is drawn across every day, so it says nothing about whether there is data. */
const GOAL_LABEL = 'Weekly goal';

export function weekLine(starts: string[], daily: Values, goal: number | null, today: string, xLabels: XLabel[]): LineChartSpec {
  const total = runningTotal(daily, starts, today);
  const top = Math.max(goal ?? 0, ...present(total));
  const s: ChartSeries[] = [{ label: 'Week so far', color: 'energy', data: total, width: 2.5, points: true }];
  if (goal !== null) s.push({ label: GOAL_LABEL, color: 'warn', data: starts.map(() => goal), width: 1 });
  return {
    kind: 'line',
    x: starts.map((_, i) => i),
    series: s,
    xRange: [-0.5, starts.length - 0.5],
    xLabels,
    yRange: [0, Math.max(10, Math.ceil((top * 1.05) / 10) * 10)],
    yUnit: 'int',
    legend: [key(s[0] as ChartSeries), ...(goal !== null ? [{ key: 'goal', label: 'Weekly goal', color: 'var(--warn)', shape: 'dash' as const }] : [])],
  };
}

export function rangeChart(metric: MetricKey, d: MetricRange, today: string): ChartSpec {
  switch (metric) {
    case 'heart_rate': {
      const s: ChartSeries[] = [
        { label: 'High', color: 'heart', data: values(d, 'high') },
        { label: 'Resting', color: 'link', data: values(d, 'resting') },
      ];
      return line(d, s, { legend: s.map(key) });
    }
    case 'stress': {
      const s: ChartSeries[] = [{ label: 'Average stress', color: 'warn', data: values(d, 'average') }];
      const average = line(d, s, { yRange: [0, 100], legend: s.map(key) });
      // Minutes per level per day (the figures are seconds); a week bucket shows the mean day.
      const parts: StackPart[] = MEASURED_LEVELS.map((level) => ({
        key: level,
        color: LEVEL_COLOR[level],
        values: values(d, `${level}_s`).map((v) => (v === null ? null : v / 60)),
      }));
      return {
        kind: 'stacked',
        parts,
        ...barLabels(d),
        average,
        legend: MEASURED_LEVELS.map((level) => ({ key: level, label: `${LEVEL_NAME[level]} ${LEVEL_RANGE[level]}`, color: LEVEL_COLOR[level] })),
      };
    }
    case 'energy': {
      // Each day is a floating range from its lowest to its highest value.
      const high = values(d, 'high');
      const low = values(d, 'low');
      const s: ChartSeries[] = [
        { label: 'Daily high', color: 'energy', data: high, noLine: true },
        { label: 'Daily low', color: 'warn', data: low, noLine: true },
      ];
      const band: BandSeries = { lo: low, hi: high };
      return line(d, s, {
        yRange: [0, 100],
        band,
        legend: [...s.map((x) => ({ key: x.label, label: x.label, color: SWATCH[x.color], shape: 'dot' as const })), { key: 'range', label: 'Range', color: 'var(--band)', outlined: true }],
      });
    }
    case 'steps': {
      const steps = values(d, 'steps');
      const daily = d.bucket === 'day';
      const goal = daily ? (d.goal ?? undefined) : undefined;
      return {
        kind: 'bars',
        values: steps,
        ...barLabels(d),
        ...(goal !== undefined ? { goal } : {}),
        color: 'var(--steps)',
        ...(goal !== undefined ? { belowGoalColor: 'var(--steps-dim)' } : {}),
        yUnit: 'int',
        legend:
          goal !== undefined
            ? [
                { key: 'met', label: 'Goal reached', color: 'var(--steps)' },
                { key: 'below', label: 'Below goal', color: 'var(--steps-dim)' },
                { key: 'goal', label: 'Goal', color: 'var(--text-2)', shape: 'dash' },
              ]
            : [{ key: 'steps', label: daily ? 'Steps' : 'Steps per week', color: 'var(--steps)' }],
      };
    }
    case 'intensity_minutes': {
      const total = creditedMinutes(d);
      // One calendar week: the total so far, against the weekly goal.
      if (d.range === '7d') return weekLine(bucketStarts(d), total, d.goal, today, bucketAxis(d));
      const weekly = d.bucket === 'week';
      const goal = weekly ? (d.goal ?? undefined) : undefined;
      return {
        kind: 'bars',
        values: total,
        ...barLabels(d),
        ...(goal !== undefined ? { goal } : {}),
        color: 'var(--energy)',
        ...(goal !== undefined ? { belowGoalColor: 'var(--link)' } : {}),
        yUnit: 'int',
        legend:
          goal !== undefined
            ? [
                { key: 'met', label: 'Goal reached', color: 'var(--energy)' },
                { key: 'below', label: 'Below goal', color: 'var(--link)' },
                { key: 'goal', label: 'Weekly goal', color: 'var(--text-2)', shape: 'dash' },
              ]
            : [{ key: 'min', label: 'Credited minutes', color: 'var(--energy)' }],
      };
    }
    case 'training_load': {
      const s: ChartSeries[] = [
        { label: 'Acute load', color: 'link', data: values(d, 'acute') },
        { label: 'Chronic load', color: 'steps', data: values(d, 'chronic') },
      ];
      const band: [number, number] | undefined = d.target_low !== null && d.target_high !== null ? [d.target_low, d.target_high] : undefined;
      return line(d, s, { ...(band ? { band } : {}), legend: [...s.map(key), ...(band ? [{ key: 'target', label: 'Target range', color: 'var(--band)', outlined: true }] : [])] });
    }
    case 'vo2_max': {
      const s: ChartSeries[] = [{ label: 'VO2 max', color: 'energy', data: values(d, 'vo2_max') }];
      return line(d, s, { yUnit: 'decimal', legend: s.map(key) });
    }
    case 'respiration': {
      const s: ChartSeries[] = [
        { label: 'Asleep', color: 'sleep', data: values(d, 'asleep') },
        { label: 'Awake', color: 'link', data: values(d, 'awake') },
      ];
      return line(d, s, { yUnit: 'decimal', legend: s.map(key) });
    }
    case 'spo2': {
      const low = values(d, 'low');
      const s: ChartSeries[] = [
        { label: 'Average', color: 'link', data: values(d, 'average') },
        { label: 'Lowest', color: 'warn', data: low },
      ];
      return line(d, s, { yRange: [percentFloor(low), 100], legend: s.map(key) });
    }
  }
}

/** Whether the chart has anything to draw. */
export function chartHasData(c: ChartSpec): boolean {
  if (c.kind === 'bars') return present(c.values).length > 0;
  if (c.kind === 'stacked') return c.parts.some((p) => present(p.values).length > 0) || chartHasData(c.average);
  return c.series.some((s) => s.label !== GOAL_LABEL && present(s.data).length > 0);
}

// ---- list -------------------------------------------------------------------------------------------

export interface RangeRow {
  /** The first day of the bucket. */
  start: string;
  title: string;
  sub: string | null;
  value: string | null;
  valueSub: string | null;
  /** Where tapping it leads; `null` when the row is not a link. */
  target: RangeTarget | null;
}

const pct = (v: number | null, goal: number | null): string | null => (v === null || goal === null || goal <= 0 ? null : `${Math.round((v / goal) * 100)}% of goal`);
const join = (...p: (string | null)[]): string | null => {
  const t = p.filter((x): x is string => !!x);
  return t.length ? t.join(' · ') : null;
};
const one = (v: number | null): string | null => fmtFixed(v, 1);

/** The two figures of a row: the main one on the right and the line under it. */
function figures(metric: MetricKey, d: MetricRange, i: number): { value: string | null; valueSub: string | null; sub: string | null } {
  const at = (k: string): number | null => d.series.find((s) => s.key === k)?.points[i]?.value ?? null;
  const day = d.bucket === 'day';
  switch (metric) {
    case 'heart_rate':
      return { value: withUnit(fmtInt(at('resting')), 'bpm'), valueSub: withUnit(fmtInt(at('high')), 'bpm high'), sub: null };
    case 'stress':
      return { value: fmtInt(at('average')), valueSub: null, sub: null };
    case 'energy': {
      const hi = fmtInt(at('high'));
      const lo = fmtInt(at('low'));
      return { value: hi === null ? null : `${hi} high`, valueSub: lo === null ? null : `${lo} low`, sub: null };
    }
    case 'steps':
      return { value: fmtInt(at('steps')), valueSub: null, sub: day ? pct(at('steps'), d.goal) : null };
    case 'intensity_minutes': {
      const m = at('moderate');
      const v = at('vigorous');
      const c = credited(m, v);
      return {
        value: withUnit(fmtInt(c), 'min'),
        valueSub: day ? null : pct(c, d.goal),
        sub: join(m === null ? null : `${fmtInt(m)} moderate`, v === null ? null : `${fmtInt(v)} vigorous`),
      };
    }
    case 'training_load':
      return { value: fmtInt(at('acute')), valueSub: withUnit(fmtInt(at('chronic')), 'chronic'), sub: null };
    case 'vo2_max':
      return { value: one(at('vo2_max')), valueSub: null, sub: null };
    case 'respiration':
      return { value: withUnit(one(at('asleep')), 'asleep'), valueSub: withUnit(one(at('awake')), 'awake'), sub: null };
    case 'spo2':
      return { value: at('average') === null ? null : `${fmtInt(at('average'))}%`, valueSub: at('low') === null ? null : `${fmtInt(at('low'))}% lowest`, sub: null };
  }
}

/** One row per bucket that has a value, newest first. */
export function rangeRows(def: MetricDef, d: MetricRange, today: string): RangeRow[] {
  const starts = bucketStarts(d);
  const rows: RangeRow[] = [];
  starts.forEach((start, i) => {
    if (!d.series.some((s) => !isMissing(s.points[i]?.value))) return;
    const f = figures(def.key, d, i);
    const day = d.bucket === 'day';
    rows.push({
      start,
      title: day ? fmtDay(start, { weekday: 'long' }) : fmtRunDates(start, bucketEnd(d, start)),
      sub: day ? join(fmtMonthDay(start), f.sub) : f.sub,
      value: f.value,
      valueSub: f.valueSub,
      target: drillTarget(def, d.bucket, start, today),
    });
  });
  return rows.reverse();
}
