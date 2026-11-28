import type { CSSProperties, ReactNode } from 'react';
import type { StageSegment } from '../../api/generated/StageSegment';
import type { StressBlock } from '../../api/generated/StressBlock';
import type { StressSample } from '../../api/generated/StressSample';
import type { Today } from '../../api/generated';
import { ProgressBar, RangeBar, StageBar, StatusWord } from '../../components';
import { addDays, fmtShortDay } from '../../lib/dates';
import { fmtFixed, fmtInt, isMissing } from '../../lib/format';
import { LEVEL_COLOR as STRESS_COLOR, LEVEL_NAME as STRESS_NAME, MEASURED_LEVELS, stressLevelOf, type MeasuredLevel } from '../../lib/stress';
import { statusColor } from '../../lib/status';
import { useHrv } from '../health/queries';
import { useMetricRange } from '../metrics/queries';
import { dayLetter, fmtSeconds, fmtTime, hrvStatusLabel } from './format';
import { Sparkline } from './homeCharts';
import { Glyph } from './glyphs';

// The pictures of the phone's "At a glance" tiles. Each one is an image for assistive technology with
// a sentence that carries the exact figures; the parts inside it are only drawing. The geometry
// (angles, segments, buckets) is in plain functions so the tests can check it without a DOM.

/** What a tile says in the place of its picture when there is nothing to draw. */
export function Hint({ children }: { children: ReactNode }) {
  return <p className="tile__hint">{children}</p>;
}

// ---------------------------------------------------------------- geometry

const clamp01 = (n: number): number => (Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : 0);

/** A 270 degree gauge opens at the bottom: it starts at 7:30 on the clock and ends at 4:30. */
export const GAUGE_START = -135;
export const GAUGE_SWEEP = 270;

/** Angle in degrees, 0 at the top and growing clockwise, of a position along the gauge (0 to 1). */
export function gaugeAngle(fraction: number): number {
  return GAUGE_START + GAUGE_SWEEP * clamp01(fraction);
}

/** A point on a circle; 0 degrees is straight up, angles grow clockwise. */
export function polar(cx: number, cy: number, r: number, degrees: number): { x: number; y: number } {
  const a = (degrees * Math.PI) / 180;
  return { x: cx + r * Math.sin(a), y: cy - r * Math.cos(a) };
}

const f1 = (n: number): string => n.toFixed(2);

/** SVG path of an arc from one angle to another, clockwise. */
export function arcPath(cx: number, cy: number, r: number, from: number, to: number): string {
  const a = polar(cx, cy, r, from);
  const b = polar(cx, cy, r, to);
  return `M${f1(a.x)} ${f1(a.y)} A${r} ${r} 0 ${to - from > 180 ? 1 : 0} 1 ${f1(b.x)} ${f1(b.y)}`;
}

export interface DialSegment {
  key: string;
  /** Start and end along the dial, 0 to 1. */
  from: number;
  to: number;
  color: string;
}

export interface DialArc {
  key: string;
  color: string;
  from: number;
  to: number;
}

/**
 * The arcs to draw for the segments of a dial: angles in degrees, with half a gap cut off every edge
 * that touches a neighbour (and at both ends of a closed ring).
 */
export function dialArcs(segments: DialSegment[], start: number, sweep: number, gap: number, closed: boolean): DialArc[] {
  const shown = segments.filter((s) => s.to > s.from);
  return shown.map((s, i) => {
    const trimStart = closed || i > 0 ? gap / 2 : 0;
    const trimEnd = closed || i < shown.length - 1 ? gap / 2 : 0;
    const from = start + sweep * s.from + trimStart;
    const to = Math.max(from + 0.5, start + sweep * s.to - trimEnd);
    return { key: s.key, color: s.color, from, to };
  });
}

// ---------------------------------------------------------------- a dial: gauge or ring

interface DialProps {
  /** What the picture shows, with the exact figures. */
  label: string;
  segments: DialSegment[];
  /** Position of the marker along the dial, 0 to 1; null for none. */
  marker?: number | null;
  /** A closed ring (360 degrees) instead of the 270 degree gauge. */
  ring?: boolean;
  size?: number;
  thickness?: number;
  /** The big text in the middle and the small one under it. */
  value?: string | null;
  sub?: string | null;
  valueSize?: number;
}

/** A gauge (270 degrees, open at the bottom) or a ring, drawn in coloured segments with an optional dot marker. */
export function Dial({ label, segments, marker = null, ring = false, size = 120, thickness = 10, value, sub, valueSize }: DialProps) {
  const mid = size / 2;
  const r = mid - thickness / 2 - (marker === null ? 2 : 8);
  const start = ring ? 0 : GAUGE_START;
  const sweep = ring ? 360 : GAUGE_SWEEP;
  const arcs = dialArcs(segments, start, sweep, ring ? 3 : 2.5, ring);
  const whole = ring && arcs.length === 1;
  const dot = marker === null ? null : polar(mid, mid, r, ring ? 360 * clamp01(marker) : gaugeAngle(marker));
  const big = valueSize ?? size * 0.25;
  return (
    <svg className="tv-dial" width={size} height={size} viewBox={`0 0 ${size} ${size}`} {...(label ? { role: 'img', 'aria-label': label } : { 'aria-hidden': true })}>
      {whole ? (
        <circle cx={mid} cy={mid} r={r} fill="none" strokeWidth={thickness} style={{ stroke: arcs[0]!.color }} />
      ) : (
        arcs.map((a) => <path key={a.key} d={arcPath(mid, mid, r, a.from, a.to)} fill="none" strokeWidth={thickness} style={{ stroke: a.color }} />)
      )}
      {dot ? <circle cx={dot.x} cy={dot.y} r={thickness * 0.7} strokeWidth={3} style={{ fill: 'var(--text)', stroke: 'var(--card)' }} /> : null}
      {value ? (
        <text x={mid} y={mid + big * 0.35 - (sub ? size * 0.03 : 0)} textAnchor="middle" fontFamily="var(--font-display)" fontWeight={600} fontSize={big} style={{ fill: 'var(--text)' }}>
          {value}
        </text>
      ) : null}
      {sub ? (
        <text x={mid} y={mid + big * 0.35 + size * 0.14} textAnchor="middle" fontFamily="var(--font-body)" fontSize={Math.max(11, size * 0.1)} style={{ fill: 'var(--text-2)' }}>
          {sub}
        </text>
      ) : null}
    </svg>
  );
}

/** A progress ring: the track, then an arc of `fraction` of the circle with round ends. */
export function ProgressDial({ label, fraction, color, size = 104, thickness = 10, value, sub, valueSize }: { label: string; fraction: number; color: string; size?: number; thickness?: number; value: string; sub?: string; valueSize?: number }) {
  const mid = size / 2;
  const r = mid - thickness / 2 - 2;
  const c = 2 * Math.PI * r;
  const frac = clamp01(fraction);
  const big = valueSize ?? size * 0.26;
  return (
    <svg className="tv-dial" width={size} height={size} viewBox={`0 0 ${size} ${size}`} role="img" aria-label={label}>
      <circle cx={mid} cy={mid} r={r} fill="none" strokeWidth={thickness} style={{ stroke: 'var(--track)' }} />
      {frac > 0 ? (
        <circle cx={mid} cy={mid} r={r} fill="none" strokeWidth={thickness} strokeLinecap="round" strokeDasharray={`${(c * frac).toFixed(1)} ${c.toFixed(1)}`} transform={`rotate(-90 ${mid} ${mid})`} style={{ stroke: color }} />
      ) : null}
      <text x={mid} y={mid + big * 0.35 - (sub ? 5 : 0)} textAnchor="middle" fontFamily="var(--font-display)" fontWeight={600} fontSize={big} style={{ fill: 'var(--text)' }}>
        {value}
      </text>
      {sub ? (
        <text x={mid} y={mid + big * 0.35 + 11} textAnchor="middle" fontFamily="var(--font-body)" fontSize={11} style={{ fill: 'var(--text-2)' }}>
          {sub}
        </text>
      ) : null}
    </svg>
  );
}

// ---------------------------------------------------------------- heart rate

export interface HrBand {
  zone: 1 | 2 | 3 | 4 | 5;
  /** The heart rate where the band starts and ends. */
  from: number;
  to: number;
}

/** Five equal bands between the day's lowest and highest heart rate; `null` when the day has no range. */
export function hrBands(low: number, high: number): HrBand[] | null {
  if (!Number.isFinite(low) || !Number.isFinite(high) || high <= low) return null;
  const step = (high - low) / 5;
  return ([1, 2, 3, 4, 5] as const).map((zone) => ({ zone, from: low + step * (zone - 1), to: low + step * zone }));
}

/** The newest 5-minute reading of the day. */
export function latestValue(series: { value: number | null }[]): number | null {
  for (let i = series.length - 1; i >= 0; i -= 1) {
    const v = series[i]!.value;
    if (v !== null && Number.isFinite(v)) return v;
  }
  return null;
}

/** Heart rate now on a gauge of today's range, in five bands from the calmest to the hardest. */
export function HeartRateVisual({ today }: { today: Today }) {
  const h = today.heart_rate;
  const values = h.day.series.map((p) => p.value).filter((v): v is number => v !== null && Number.isFinite(v));
  const now = latestValue(h.day.series);
  const low = isMissing(h.day.low_bpm) ? (values.length ? Math.min(...values) : null) : h.day.low_bpm;
  const high = isMissing(h.day.high_bpm) ? (values.length ? Math.max(...values) : null) : h.day.high_bpm;
  const bands = now === null || low === null || high === null ? null : hrBands(Math.min(low, now), Math.max(high, now));
  if (now === null || low === null || high === null || !bands) return <Hint>No heart rate readings yet today.</Hint>;
  const lo = bands[0]!.from;
  const hi = bands[4]!.to;
  return (
    <Dial
      label={`Heart rate now ${fmtInt(now)} bpm. Today's range is ${fmtInt(low)} to ${fmtInt(high)} bpm, in five equal bands.`}
      segments={bands.map((b) => ({ key: `z${b.zone}`, from: (b.from - lo) / (hi - lo), to: (b.to - lo) / (hi - lo), color: `var(--zone-${b.zone})` }))}
      marker={(now - lo) / (hi - lo)}
      value={fmtInt(now)}
      sub="bpm now"
    />
  );
}

// ---------------------------------------------------------------- stress

export interface StressSlice {
  level: MeasuredLevel;
  seconds: number;
}

/** The time at rest, low, medium and high stress, in that order, without the levels that have no time. */
export function stressSlices(s: Pick<StressBlock, 'rest_s' | 'low_s' | 'medium_s' | 'high_s'>): StressSlice[] {
  const seconds: Record<MeasuredLevel, number | null> = { rest: s.rest_s, low: s.low_s, medium: s.medium_s, high: s.high_s };
  return MEASURED_LEVELS.flatMap((level) => {
    const v = seconds[level];
    return v !== null && Number.isFinite(v) && v > 0 ? [{ level, seconds: v }] : [];
  });
}

/** Ring segments (0 to 1 of the circle) for the slices. */
export function stressSegments(slices: StressSlice[]): DialSegment[] {
  const total = slices.reduce((a, s) => a + s.seconds, 0);
  if (total <= 0) return [];
  let at = 0;
  return slices.map((s) => {
    const from = at;
    at += s.seconds / total;
    return { key: s.level, from, to: at, color: STRESS_COLOR[s.level] };
  });
}

export const DAY_BUCKETS = 288;

/** Minutes after midnight in the owner's time zone; `null` for an unreadable time. */
export function minuteOfDay(iso: string, timeZone: string): number | null {
  const t = fmtTime(iso, timeZone);
  if (!t) return null;
  const [h, m] = t.split(':').map(Number);
  return h === undefined || m === undefined || Number.isNaN(h) || Number.isNaN(m) ? null : h * 60 + m;
}

export type BucketLevel = MeasuredLevel | 'activity';

/**
 * Today's stress readings in 5-minute buckets from midnight (288 of them): the level most of the
 * readings of the bucket have (the higher one on a tie); a bucket with only movement is `activity`;
 * a bucket without readings is `null`.
 */
export function stressBuckets(series: StressSample[], timeZone: string): (BucketLevel | null)[] {
  const counts: Record<BucketLevel, number>[] = Array.from({ length: DAY_BUCKETS }, () => ({ rest: 0, low: 0, medium: 0, high: 0, activity: 0 }));
  for (const sample of series) {
    const minute = minuteOfDay(sample.time, timeZone);
    if (minute === null) continue;
    const slot = Math.min(DAY_BUCKETS - 1, Math.floor(minute / 5));
    const level: BucketLevel = sample.stress !== null && Number.isFinite(sample.stress) ? stressLevelOf(sample.stress) : sample.level === 'activity' ? 'activity' : sample.level;
    counts[slot]![level] += 1;
  }
  return counts.map((c) => {
    let best: MeasuredLevel | null = null;
    let top = 0;
    for (const level of MEASURED_LEVELS) {
      if (c[level] > 0 && c[level] >= top) {
        best = level;
        top = c[level];
      }
    }
    return best ?? (c.activity > 0 ? 'activity' : null);
  });
}

const HOURS = ['00', '06', '12', '18', '24'];

/** A thin strip of the day, midnight to midnight, one bar per 5 minutes coloured by its level. */
function DayStrip({ buckets }: { buckets: (BucketLevel | null)[] }) {
  return (
    <div className="tv-strip" aria-hidden="true">
      <svg viewBox={`0 0 ${DAY_BUCKETS} 10`} preserveAspectRatio="none" width="100%" height={10} shapeRendering="crispEdges">
        {buckets.map((level, i) => (level === null ? null : <rect key={i} x={i} y={0} width={1.05} height={10} style={{ fill: STRESS_COLOR[level] }} />))}
      </svg>
      <div className="tv-strip__hours">
        {HOURS.map((h) => (
          <span key={h}>{h}</span>
        ))}
      </div>
    </div>
  );
}

/** The share of the day at rest, low, medium and high stress as a ring, and the day as a strip under it. */
export function StressVisual({ today }: { today: Today }) {
  const s = today.stress;
  if (isMissing(s.average)) return <Hint>No stress readings yet today.</Hint>;
  const slices = stressSlices(s);
  const buckets = stressBuckets(s.series, today.timezone);
  const hasStrip = buckets.some((b) => b !== null);
  const words = slices.map((x) => `${STRESS_NAME[x.level].toLowerCase()} ${fmtSeconds(x.seconds)}`).join(', ');
  return (
    <div className="tv-stack" role="img" aria-label={`Average stress ${fmtInt(s.average)} out of 100${words ? `. Time today: ${words}` : ''}${hasStrip ? '. The strip shows the day from 00 to 24, coloured by level.' : '.'}`}>
      <Dial
        ring
        size={slices.length ? (hasStrip ? 84 : 104) : 84}
        thickness={9}
        segments={slices.length ? stressSegments(slices) : [{ key: 'none', from: 0, to: 1, color: 'var(--track)' }]}
        value={fmtInt(s.average)}
        valueSize={24}
        label=""
      />
      {hasStrip ? <DayStrip buckets={buckets} /> : null}
    </div>
  );
}

// ---------------------------------------------------------------- sleep

/** The four stages the chart shows; a stretch the watch could not measure is left blank. */
export type StageKey = 'awake' | 'rem' | 'light' | 'deep';
const isStage = (s: string): s is StageKey => s === 'awake' || s === 'rem' || s === 'light' || s === 'deep';

/** Row of each stage in the chart, from the top. */
export const STAGE_ROW: Record<StageKey, number> = { awake: 0, rem: 1, light: 2, deep: 3 };

export interface StageBlock {
  stage: StageKey;
  /** Start and width as a share of the night, 0 to 1. */
  x: number;
  w: number;
}

/** The blocks of a night's stage chart; parts outside the night are cut off. */
export function stageBlocks(stages: StageSegment[], start: string, end: string): StageBlock[] {
  const t0 = Date.parse(start);
  const t1 = Date.parse(end);
  const span = t1 - t0;
  if (!Number.isFinite(span) || span <= 0) return [];
  return stages.flatMap((s) => {
    if (!isStage(s.stage)) return [];
    const a = Math.max(t0, Date.parse(s.start));
    const b = Math.min(t1, Date.parse(s.end));
    if (!Number.isFinite(a) || !Number.isFinite(b) || b <= a) return [];
    return [{ stage: s.stage, x: (a - t0) / span, w: (b - a) / span }];
  });
}

const STAGE_COLOR: Record<StageKey, string> = { awake: 'var(--stage-awake)', rem: 'var(--stage-rem)', light: 'var(--stage-light)', deep: 'var(--stage-deep)' };

/** The night as blocks at four heights (awake on top, deep at the bottom), bed and wake time under the ends. */
export function SleepVisual({ today }: { today: Today }) {
  const s = today.sleep;
  if (!s) return <Hint>The watch recorded no sleep for last night.</Hint>;
  if (!s.stages_available) return <Hint>No sleep stages for last night.</Hint>;
  const tz = today.timezone;
  const bed = fmtTime(s.start, tz);
  const wake = fmtTime(s.end, tz);
  const totals = `deep ${fmtSeconds(s.deep_s) ?? 'unavailable'}, light ${fmtSeconds(s.light_s) ?? 'unavailable'}, REM ${fmtSeconds(s.rem_s) ?? 'unavailable'}, awake ${fmtSeconds(s.awake_s) ?? 'unavailable'}`;
  const times = bed && wake ? ` From ${bed} to ${wake}.` : '';
  const blocks = stageBlocks(s.stages, s.start, s.end);
  if (blocks.length === 0) {
    return (
      <div className="tv-stack tv-stack--wide">
        <StageBar
          height={12}
          label={`Sleep stages: ${totals}.${times}`}
          segments={[
            { key: 'deep', label: 'Deep', value: s.deep_s, color: STAGE_COLOR.deep },
            { key: 'light', label: 'Light', value: s.light_s, color: STAGE_COLOR.light },
            { key: 'rem', label: 'REM', value: s.rem_s, color: STAGE_COLOR.rem },
            { key: 'awake', label: 'Awake', value: s.awake_s, color: STAGE_COLOR.awake },
          ]}
        />
        {bed && wake ? <Ends from={bed} to={wake} /> : null}
      </div>
    );
  }
  const ROW = 18;
  const GAP = 5;
  const H = ROW * 4 + GAP * 3;
  return (
    <div className="tv-stack tv-stack--wide" role="img" aria-label={`Sleep stages: ${totals}.${times}`}>
      <svg viewBox={`0 0 100 ${H}`} preserveAspectRatio="none" width="100%" height={78} shapeRendering="crispEdges" aria-hidden="true">
        {blocks.map((b, i) => (
          <rect key={i} x={b.x * 100} y={STAGE_ROW[b.stage] * (ROW + GAP)} width={Math.max(0.5, b.w * 100)} height={ROW} rx={0.6} style={{ fill: STAGE_COLOR[b.stage] }} />
        ))}
      </svg>
      {bed && wake ? <Ends from={bed} to={wake} /> : null}
    </div>
  );
}

function Ends({ from, to }: { from: string; to: string }) {
  return (
    <div className="tv-ends" aria-hidden="true">
      <span>{from}</span>
      <span>{to}</span>
    </div>
  );
}

// ---------------------------------------------------------------- HRV

export interface HrvZone {
  key: 'low' | 'unbalanced-low' | 'balanced' | 'unbalanced-high';
  /** Start and end along the bar, 0 to 1. */
  from: number;
  to: number;
  color: string;
}

export interface HrvScale {
  zones: HrvZone[];
  /** Where a value in ms sits along the bar (0 to 1, held to the ends). */
  at: (ms: number) => number;
}

/**
 * The bar of the overnight HRV status. The balanced zone is the watch's own range; the two unbalanced
 * zones are half as wide as it on each side, and the low zone fills the rest. Only the balanced edges
 * are the watch's numbers, the outer zones are there for orientation.
 */
export function hrvScale(low: number, high: number): HrvScale | null {
  const w = high - low;
  if (!Number.isFinite(low) || !Number.isFinite(high) || w <= 0) return null;
  const min = low - w;
  const max = high + w / 2;
  const at = (ms: number) => clamp01((ms - min) / (max - min));
  return {
    at,
    zones: [
      { key: 'low', from: 0, to: at(low - w / 2), color: 'var(--hrv-low)' },
      { key: 'unbalanced-low', from: at(low - w / 2), to: at(low), color: 'var(--hrv-unbalanced)' },
      { key: 'balanced', from: at(low), to: at(high), color: 'var(--hrv-balanced)' },
      { key: 'unbalanced-high', from: at(high), to: 1, color: 'var(--hrv-unbalanced)' },
    ],
  };
}

export type HrvDot = { date: string; status: string | null };

/** The nights of the last 4 weeks, oldest first: the status of each, `null` for a night without one. */
export function hrvDots(days: { date: string; status: string | null }[], to: string, count = 28): HrvDot[] {
  const byDate = new Map(days.map((d) => [d.date, d.status]));
  return Array.from({ length: count }, (_, i) => {
    const date = addDays(to, i - (count - 1));
    return { date, status: byDate.get(date) ?? null };
  });
}

/** Zone bar with a dot marker: the zones side by side, the marker where the value sits. */
export function ZoneBar({ zones, marker }: { zones: { key: string; from: number; to: number; color: string }[]; marker: number | null }) {
  return (
    <div className="tv-zones" aria-hidden="true">
      <div className="tv-zones__bar">
        {zones.map((z) => (
          <span key={z.key} style={{ left: `${z.from * 100}%`, width: `${(z.to - z.from) * 100}%`, background: z.color }} />
        ))}
      </div>
      {marker === null ? null : <i className="tv-zones__marker" style={{ left: `${clamp01(marker) * 100}%` }} />}
    </div>
  );
}

/** The HRV status word, the 7-night average on the status bar, and a dot for each of the last 28 nights. */
export function HrvVisual({ today }: { today: Today }) {
  const h = today.hrv;
  const from = addDays(today.date, -27);
  const q = useHrv(from, today.date);
  const avg = h.weekly_avg_ms;
  const scale = !isMissing(h.baseline_low_ms) && !isMissing(h.baseline_high_ms) ? hrvScale(h.baseline_low_ms, h.baseline_high_ms) : null;
  const status = hrvStatusLabel(h.status);
  if (isMissing(avg) && !status) return <Hint>Wear your watch to bed to see this.</Hint>;
  const dots = hrvDots((q.data?.days ?? []).map((d) => ({ date: d.date, status: d.status })), today.date);
  const counts = { balanced: 0, unbalanced: 0, low: 0, poor: 0, none: 0 };
  for (const d of dots) counts[(d.status ?? 'none') as keyof typeof counts] += 1;
  const nights = [
    counts.balanced ? `${counts.balanced} balanced` : null,
    counts.unbalanced ? `${counts.unbalanced} unbalanced` : null,
    counts.low ? `${counts.low} low` : null,
    counts.poor ? `${counts.poor} poor` : null,
  ].filter((x): x is string => x !== null);
  const label = [
    status ? `Status ${status}.` : null,
    isMissing(avg) ? null : `7-night average ${fmtInt(avg)} ms.`,
    scale && !isMissing(h.baseline_low_ms) && !isMissing(h.baseline_high_ms) ? `Balanced range ${fmtInt(h.baseline_low_ms)} to ${fmtInt(h.baseline_high_ms)} ms.` : null,
    q.data ? (nights.length ? `Last 4 weeks: ${nights.join(', ')}, ${counts.none} without a status.` : 'Last 4 weeks: no statuses.') : null,
  ]
    .filter((x): x is string => x !== null)
    .join(' ');
  return (
    <div className="tv-hrv" role="img" aria-label={label}>
      <span className="tv-hrv__status">{status ? <StatusWord type="hrv" value={h.status}>{status}</StatusWord> : null}</span>
      {scale ? <ZoneBar zones={scale.zones} marker={isMissing(avg) ? null : scale.at(avg)} /> : <span className="tv-hrv__none">Your balanced range is unavailable.</span>}
      <div className="tv-dots" aria-hidden="true">
        {dots.map((d) => {
          const color = statusColor('hrv', d.status);
          return <i key={d.date} className={color ? 'tv-dots__dot' : 'tv-dots__dot tv-dots__dot--none'} style={color ? ({ background: color } as CSSProperties) : undefined} />;
        })}
      </div>
      <span className="tv-hrv__foot" aria-hidden="true">
        Last 4 weeks
      </span>
    </div>
  );
}

// ---------------------------------------------------------------- readiness

/** The five levels along the gauge, as shares of the 0 to 100 score: 0-24, 25-49, 50-74, 75-94, 95-100. */
export const READINESS_EDGES = [0, 25, 50, 75, 95, 100] as const;
const READINESS_KEYS = ['poor', 'low', 'moderate', 'high', 'prime'] as const;

export function readinessSegments(colors: Record<(typeof READINESS_KEYS)[number], string>): DialSegment[] {
  return READINESS_KEYS.map((key, i) => ({ key, from: READINESS_EDGES[i]! / 100, to: READINESS_EDGES[i + 1]! / 100, color: colors[key] }));
}

/** The readiness score on a gauge of the five levels. */
export function ReadinessVisual({ score, colors, label }: { score: number; colors: Record<(typeof READINESS_KEYS)[number], string>; label: string }) {
  return <Dial label={label} segments={readinessSegments(colors)} marker={score / 100} value={fmtInt(score)} sub="of 100" />;
}

// ---------------------------------------------------------------- training status

/** A disc in the status colour with the status glyph cut out of it. */
export function StatusDisc({ color, label }: { color: string; label: string }) {
  return (
    <span className="tv-disc" role="img" aria-label={label} style={{ background: color }}>
      <Glyph name="status" size={34} />
    </span>
  );
}

// ---------------------------------------------------------------- steps

/** The last 7 days of steps: a bar a day, in full colour when the goal was reached, a letter under each, and the goal bar. */
export function StepsVisual({ today }: { today: Today }) {
  const q = useMetricRange('steps', '7d', today.date);
  const s = today.steps;
  const points = q.data?.series.find((x) => x.key === 'steps')?.points ?? [];
  const goal = s.goal > 0 ? s.goal : null;
  const top = Math.max(1, goal ?? 0, ...points.map((p) => p.value ?? 0));
  const week = points.map((p) => `${fmtShortDay(p.date)} ${p.value === null ? 'unavailable' : fmtInt(p.value)}`).join(', ');
  return (
    <div className="tv-stack tv-stack--wide tv-steps">
      <div className="tv-bars" role="img" aria-label={points.length ? `Steps over the last 7 days: ${week}. Days that reached the goal are in full colour.` : 'Steps over the last 7 days are not available.'}>
        <div className="tv-bars__plot">
          {(points.length ? points : Array.from({ length: 7 }, () => null)).map((p, i) => {
            const v = p?.value ?? null;
            const has = v !== null && v > 0;
            const reached = has && goal !== null && v >= goal;
            return (
              <span className="tv-bars__slot" key={i}>
                <span className={has ? 'tv-bars__bar' : 'tv-bars__bar tv-bars__bar--empty'} style={has ? { height: `${Math.max(8, (v / top) * 100)}%`, background: goal === null || reached ? 'var(--steps)' : 'var(--steps-dim)' } : undefined} />
              </span>
            );
          })}
        </div>
        <div className="tv-bars__letters">
          {points.map((p) => (
            <span key={p.date}>{dayLetter(p.date)}</span>
          ))}
        </div>
      </div>
      <ProgressBar value={s.steps} max={s.goal} height={8} label={`${fmtInt(s.steps) ?? 'unavailable'} of ${fmtInt(s.goal)} steps`} />
    </div>
  );
}

// ---------------------------------------------------------------- intensity, load, recovery, VO2

export function IntensityVisual({ today }: { today: Today }) {
  const m = today.intensity_minutes;
  if (isMissing(m.total_min) || m.goal_min <= 0) return <Hint>No intensity minutes this week.</Hint>;
  return (
    <ProgressDial
      label={`Intensity minutes this week: ${fmtInt(m.total_min)} of the ${fmtInt(m.goal_min)} minute goal`}
      fraction={m.total_min / m.goal_min}
      color="var(--energy)"
      value={String(fmtInt(m.total_min))}
      sub="min"
      size={108}
      valueSize={28}
    />
  );
}

export function LoadVisual({ today }: { today: Today }) {
  const l = today.training_load;
  if (isMissing(l.seven_day_load)) return <Hint>No training load from the watch this week.</Hint>;
  if (isMissing(l.target_min) || isMissing(l.target_max)) return <Hint>No target range from the watch yet.</Hint>;
  const max = Math.max(l.target_max, l.seven_day_load, 1) * 1.4;
  return (
    <div className="tv-stack tv-stack--wide tv-load">
      <RangeBar value={l.seven_day_load} min={0} max={max} target={[l.target_min, l.target_max]} label={`7-day load ${fmtInt(l.seven_day_load)}. Target range ${fmtInt(l.target_min)} to ${fmtInt(l.target_max)}.`} />
      <span className="tv-load__target" aria-hidden="true">
        Target {fmtInt(l.target_min)}–{fmtInt(l.target_max)}
      </span>
    </div>
  );
}

/** Fraction of the recovery time that is left, 0 to 1; `null` when the data cannot say. */
export function recoveryLeft(r: Today['recovery_time']): number | null {
  if (isMissing(r.remaining_s)) return null;
  if (!isMissing(r.total_s) && r.total_s > 0) return clamp01(r.remaining_s / r.total_s);
  if (!isMissing(r.percent_complete)) return clamp01(1 - r.percent_complete / 100);
  return null;
}

export function RecoveryVisual({ today }: { today: Today }) {
  const r = today.recovery_time;
  const left = recoveryLeft(r);
  if (left === null || isMissing(r.remaining_s)) return <Hint>No recovery time from the watch yet.</Hint>;
  return (
    <ProgressDial
      label={`Recovery time left ${fmtSeconds(r.remaining_s) ?? 'unavailable'}`}
      fraction={left}
      color="var(--zone-2)"
      value={r.remaining_s === 0 ? 'Done' : (fmtSeconds(r.remaining_s) ?? '')}
      size={108}
      valueSize={r.remaining_s === 0 ? 20 : 16}
    />
  );
}

export function Vo2Visual({ today }: { today: Today }) {
  const values = today.vo2max.series_3m.map((d) => d.value);
  const known = values.filter((v): v is number => v !== null && Number.isFinite(v));
  if (known.length < 2) return <Hint>Not enough readings for a trend yet.</Hint>;
  return (
    <div className="tv-spark">
      <Sparkline values={values} color="var(--zone-4)" height={80} area label={`VO2 max over the last 3 months: ${known.length} values from ${fmtFixed(Math.min(...known), 1)} to ${fmtFixed(Math.max(...known), 1)}.`} />
    </div>
  );
}

// ---------------------------------------------------------------- the quiet ones

/** Label and value rows (breathing, blood oxygen). */
export function Rows({ rows, label }: { rows: { name: string; value: ReactNode }[]; label: string }) {
  return (
    <dl className="tv-rows" aria-label={label}>
      {rows.map((r) => (
        <div key={r.name}>
          <dt>{r.name}</dt>
          <dd>{r.value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** The change in body weight: an arrow (up, down or level) and the change. The arrow is no judgement, so it has no good or bad colour. */
export function WeightChange({ kg, text }: { kg: number; text: string }) {
  const dir = kg > 0 ? 'up' : kg < 0 ? 'down' : 'level';
  return (
    <div className="tv-change" role="img" aria-label={`Change in the last 30 days: ${text} kg`}>
      <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        {dir === 'up' ? <path d="M12 19V5M6 11l6-6 6 6" /> : dir === 'down' ? <path d="M12 5v14M6 13l6 6 6-6" /> : <path d="M5 12h14M13 6l6 6-6 6" />}
      </svg>
      <span className="tv-change__value">{text} kg</span>
      <span className="tv-change__label">in 30 days</span>
    </div>
  );
}
