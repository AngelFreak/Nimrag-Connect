import type { CSSProperties, ReactNode } from 'react';
import { Unavailable } from './Unavailable';

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
}

/** Colour key. The text next to each swatch carries the meaning; colour only reinforces it. */
export function Legend({ items, grid }: { items: LegendItem[]; grid?: boolean }) {
  return (
    <ul className={grid ? 'legend legend--grid' : 'legend'}>
      {items.map((it) => (
        <li className="legend__item" key={it.key}>
          <span className="swatch" aria-hidden="true" style={{ background: it.color, border: it.outlined ? '1px solid var(--line-strong)' : undefined }} />
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
      <p>
        <Unavailable />
      </p>
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

const STRESS_COLOR: Record<StressLevel, string> = {
  rest: 'var(--stress-rest)',
  low: 'var(--stress-low)',
  medium: 'var(--stress-medium)',
  high: 'var(--heart)',
  activity: 'var(--stress-activity)',
};

/** Stress across the day: one column per time bucket, coloured by level. Pair it with a Legend. */
export function StressBars({ buckets, label }: { buckets: StressBucket[]; label: string }) {
  const slots = buckets.reduce((a, b) => a + (b.span ?? 1), 0);
  const anyData = buckets.some((b) => b.level === 'rest' || b.level === 'activity' || b.value !== null);
  if (slots === 0 || !anyData) {
    return (
      <p>
        <Unavailable />
      </p>
    );
  }
  const H = 90;
  const starts: number[] = [];
  buckets.reduce((sum, b) => {
    starts.push(sum);
    return sum + (b.span ?? 1);
  }, 0);
  return (
    <div className="chart">
      <svg viewBox={`0 0 320 ${H}`} preserveAspectRatio="none" role="img" aria-label={label} style={{ height: H }}>
        {buckets.map((b, i) => {
          const w = ((b.span ?? 1) / slots) * 320;
          const x = ((starts[i] ?? 0) / slots) * 320;
          if (b.level === 'activity') return <rect key={i} x={x} y={0} width={w} height={H} style={{ fill: STRESS_COLOR.activity }} />;
          if (b.level === 'rest') return <rect key={i} x={x} y={H - 10} width={w} height={10} style={{ fill: STRESS_COLOR.rest }} />;
          if (b.value === null) return null;
          const h = Math.max(4, (b.value / 100) * H);
          return <rect key={i} x={x} y={H - h} width={w} height={h} style={{ fill: STRESS_COLOR[b.level] }} />;
        })}
      </svg>
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

const STAGE_Y: Record<SleepStage, number> = { awake: 0, rem: 24, light: 48, deep: 72 };
const STAGE_COLOR: Record<SleepStage, string> = {
  awake: 'var(--stage-awake)',
  rem: 'var(--stage-rem)',
  light: 'var(--stage-light)',
  deep: 'var(--stage-deep)',
};

interface StageTimelineProps {
  spans: StageSpan[];
  totalMinutes: number;
  label: string;
  /** Time labels under the chart, evenly spaced ("23:18", "01:00", ...). */
  ticks?: string[];
}

/** Sleep hypnogram: one row per stage (awake on top, deep at the bottom). */
export function StageTimeline({ spans, totalMinutes, label, ticks }: StageTimelineProps) {
  if (spans.length === 0 || totalMinutes <= 0) {
    return (
      <p>
        <Unavailable />
      </p>
    );
  }
  const x = (m: number) => (m / totalMinutes) * 320;
  return (
    <div className="chart">
      <svg viewBox="0 0 320 96" preserveAspectRatio="none" role="img" aria-label={label} style={{ height: 96 }}>
        {spans.map((s, i) => (
          <rect key={i} x={x(s.from)} y={STAGE_Y[s.stage]} width={Math.max(1.5, x(s.to) - x(s.from))} height={20} style={{ fill: STAGE_COLOR[s.stage] }} />
        ))}
      </svg>
      {ticks ? (
        <div className="chart__axis" aria-hidden="true">
          {ticks.map((t) => (
            <span key={t}>{t}</span>
          ))}
        </div>
      ) : null}
    </div>
  );
}
