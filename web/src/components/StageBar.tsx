import type { CSSProperties, ReactNode } from 'react';
import { LEVEL_COLOR as STRESS_COLOR, stressLevelOf } from '../lib/stress';
import { tickCountFor, unitTicks } from '../lib/ticks';
import { evenLabels, PlotFrame, PlotXLabels, plotTicks, type PlotTick } from './PlotFrame';
import { EmptyNote, Unavailable } from './Unavailable';

export interface LegendItem {
  key: string;
  label: ReactNode;
  color: string;
  /** Text at the end of the row ("1h 24m"); null renders "unavailable". */
  value?: ReactNode | null;
  /** Show the value slot even when empty (so a missing value reads "unavailable"). */
  hasValue?: boolean;
  /** Draw the swatch with an outline (used for the near-background "Activity" bucket). */
  outlined?: boolean;
  /**
   * Shape of the swatch, matching what the chart draws: `square` for bars and bands (default),
   * `line` for a line series, `dash` for a target or goal line, `dot` for single readings, `triangle` for a marked reading.
   */
  shape?: LegendShape;
}

export type LegendShape = 'square' | 'line' | 'dash' | 'dot' | 'triangle';

/** Colour key. The text next to each swatch carries the meaning; colour only reinforces it. */
export function Legend({ items, grid }: { items: LegendItem[]; grid?: boolean }) {
  return (
    <ul className={grid ? 'legend legend--grid' : 'legend'}>
      {items.map((it) => (
        <li className="legend__item" key={it.key}>
          <span
            className={it.shape && it.shape !== 'square' ? `swatch swatch--${it.shape}` : 'swatch'}
            aria-hidden="true"
            style={{ '--swatch': it.color, border: it.outlined ? '1px solid var(--line-strong)' : undefined } as CSSProperties}
          />
          <span className="legend__label">{it.label}</span>
          {it.hasValue || it.value !== undefined ? <span className="legend__value">{it.value === null || it.value === undefined ? <Unavailable /> : it.value}</span> : null}
        </li>
      ))}
    </ul>
  );
}

export interface StageSegment {
  key: string;
  label: string;
  /** Size of the segment (minutes, load, ...). null / undefined segments are skipped. */
  value: number | null | undefined;
  color: string;
}

interface StageBarProps {
  segments: StageSegment[];
  /** Accessible summary in words: "Sleep stages: deep 1h 24m, light 3h 58m, ...". */
  label: string;
  height?: 8 | 10 | 12;
  /** Full-scale value; defaults to the sum (a fully stacked bar). Use the goal for a goal bar. */
  total?: number;
}

/** Stacked horizontal bar: sleep stages, stress buckets, intensity minutes (moderate + vigorous). */
export function StageBar({ segments, label, height = 10, total }: StageBarProps) {
  const present = segments.filter((s) => typeof s.value === 'number' && Number.isFinite(s.value) && s.value > 0);
  const sum = total ?? present.reduce((a, s) => a + (s.value ?? 0), 0);
  if (present.length === 0 || sum <= 0) {
    return (
      <EmptyNote>No time recorded.</EmptyNote>
    );
  }
  return (
    <div className="progress" style={{ '--h': `${height}px` } as CSSProperties} role="img" aria-label={label}>
      {present.map((s) => (
        <div key={s.key} className="progress__fill" style={{ width: `${((s.value ?? 0) / sum) * 100}%`, background: s.color, borderRadius: 0 }} />
      ))}
    </div>
  );
}

export type StressLevel = 'rest' | 'low' | 'medium' | 'high' | 'activity';

export interface StressBucket {
  level: StressLevel;
  /** 0..100 stress score for the bucket; ignored for rest and activity. null leaves a gap. */
  value: number | null;
  /** Width in slots (a night of rest spans many slots). */
  span?: number;
}

interface StressBarsProps {
  buckets: StressBucket[];
  label: string;
  /** Labels under the bars, spread from the left edge to the right edge ("00", "06", "12", "18", "24"). */
  xLabels?: string[];
}

/** Stress across the day: one column per time bucket, on a 0 to 100 scale, each coloured by the level of its own value (0-25 rest, 26-50 low, 51-75 medium, 76-100 high). Pair it with a Legend. */
export function StressBars({ buckets, label, xLabels }: StressBarsProps) {
  const slots = buckets.reduce((a, b) => a + (b.span ?? 1), 0);
  const anyData = buckets.some((b) => b.level === 'rest' || b.level === 'activity' || b.value !== null);
  if (slots === 0 || !anyData) {
    return (
      <EmptyNote>Nothing recorded for this period.</EmptyNote>
    );
  }
  const H = 90;
  const starts: number[] = [];
  buckets.reduce((sum, b) => {
    starts.push(sum);
    return sum + (b.span ?? 1);
  }, 0);
  const ticks = plotTicks(unitTicks(0, 100, tickCountFor(H, 16)), 'int', (v) => 1 - v / 100);
  return (
    <div className="chart">
      <PlotFrame ticks={ticks} viewW={320} viewH={H} label={label} xAxis={xLabels?.length ? <PlotXLabels labels={evenLabels(xLabels)} /> : undefined}>
        {buckets.map((b, i) => {
          const w = ((b.span ?? 1) / slots) * 320;
          const x = ((starts[i] ?? 0) / slots) * 320;
          if (b.level === 'activity') return <rect key={i} x={x} y={0} width={w} height={H} style={{ fill: STRESS_COLOR.activity }} />;
          if (b.level === 'rest') return <rect key={i} x={x} y={H - 10} width={w} height={10} style={{ fill: STRESS_COLOR.rest }} />;
          if (b.value === null) return null;
          const h = Math.max(4, (b.value / 100) * H);
          return <rect key={i} x={x} y={H - h} width={w} height={h} style={{ fill: STRESS_COLOR[stressLevelOf(b.value)] }} />;
        })}
      </PlotFrame>
    </div>
  );
}

export type SleepStage = 'deep' | 'light' | 'rem' | 'awake';

export interface StageSpan {
  stage: SleepStage;
  /** Minutes from the start of the night. */
  from: number;
  to: number;
}

const STAGE_Y: Record<SleepStage, number> = { awake: 0, rem: 32, light: 64, deep: 96 };
const STAGE_NAME: Record<SleepStage, string> = { awake: 'Awake', rem: 'REM', light: 'Light', deep: 'Deep' };
const STAGE_ROW_H = 22;
const STAGE_H = 118;
const STAGE_COLOR: Record<SleepStage, string> = {
  awake: 'var(--stage-awake)',
  rem: 'var(--stage-rem)',
  light: 'var(--stage-light)',
  deep: 'var(--stage-deep)',
};

/** One extra measurement drawn as a line over the stage rows, on its own right-hand axis. */
export interface StageLine {
  /** What it is ("Heart rate"), for the accessible name. */
  name: string;
  color: string;
  /** Minutes from the start of the night, and the value. */
  points: { minute: number; value: number }[];
  /** One decimal on the axis (breathing rate). */
  decimals?: boolean;
}

interface StageTimelineProps {
  spans: StageSpan[];
  totalMinutes: number;
  label: string;
  /** Time labels under the chart, evenly spaced ("23:18", "01:00", ...). */
  ticks?: string[];
  /** One extra line over the night, with a right-hand axis. */
  line?: StageLine;
}

/** At most three time labels (first, middle, last): five clock times do not fit side by side at a large text size. */
function thinned(ticks: string[]): string[] {
  if (ticks.length <= 4) return ticks;
  return [0, 1, 2].map((i) => ticks[Math.round((i * (ticks.length - 1)) / 2)] as string);
}

/** Sleep hypnogram: one row per stage (awake on top, deep at the bottom). */
export function StageTimeline({ spans, totalMinutes, label, ticks, line }: StageTimelineProps) {
  if (spans.length === 0 || totalMinutes <= 0) {
    return (
      <EmptyNote>No sleep stages recorded for this night.</EmptyNote>
    );
  }
  const x = (m: number) => (m / totalMinutes) * 320;
  // The rows are the value axis: one label (and grid line) through the middle of each stage's row.
  const rows: PlotTick[] = (['awake', 'rem', 'light', 'deep'] as const).map((st) => ({ label: STAGE_NAME[st], at: (STAGE_Y[st] + STAGE_ROW_H / 2) / STAGE_H }));
  const shown = line ? line.points.filter((p) => p.minute >= 0 && p.minute <= totalMinutes && Number.isFinite(p.value)) : [];
  const drawn = line && shown.length >= 2 ? overlayLine(shown, totalMinutes, line) : null;
  return (
    <div className="chart">
      <PlotFrame
        ticks={rows}
        viewW={320}
        viewH={STAGE_H}
        label={drawn && line ? `${label}, with ${line.name.toLowerCase()} over the night` : label}
        xAxis={ticks ? <PlotXLabels labels={evenLabels(thinned(ticks))} /> : undefined}
        rightTicks={drawn?.ticks}
      >
        {spans.map((s, i) => (
          <rect key={i} x={x(s.from)} y={STAGE_Y[s.stage]} width={Math.max(1.5, x(s.to) - x(s.from))} height={STAGE_ROW_H} style={{ fill: STAGE_COLOR[s.stage] }} />
        ))}
        {drawn && line ? (
          <>
            <path d={drawn.path} fill="none" strokeWidth={4.5} strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" style={{ stroke: 'var(--card)' }} />
            <path d={drawn.path} fill="none" strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" style={{ stroke: line.color }} />
          </>
        ) : null}
      </PlotFrame>
    </div>
  );
}

/** The path of a line over the stage rows, and its own value axis: the data range with a little room. */
function overlayLine(points: { minute: number; value: number }[], totalMinutes: number, line: StageLine): { path: string; ticks: PlotTick[] } {
  const values = points.map((p) => p.value);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const pad = max - min < 1e-9 ? 1 : (max - min) * 0.12;
  const lo = min - pad;
  const hi = max + pad;
  const at = (v: number) => 1 - (v - lo) / (hi - lo);
  const path = points.map((p, i) => `${i === 0 ? 'M' : 'L'}${((p.minute / totalMinutes) * 320).toFixed(2)} ${(at(p.value) * STAGE_H).toFixed(2)}`).join(' ');
  const ticks = plotTicks(unitTicks(lo, hi, 3, line.decimals ? 'decimal' : 'int'), line.decimals ? 'decimal' : 'int', at);
  return { path, ticks };
}
