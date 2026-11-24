import { Link } from 'react-router-dom';
import type { EnergyBlock } from '../../api/generated/EnergyBlock';
import type { StressBlock } from '../../api/generated/StressBlock';
import type { StressLevel } from '../../api/generated/StressLevel';
import { PlotFrame, PlotXLabels, plotTicks, Unavailable } from '../../components';
import { fmtInt } from '../../lib/format';
import { fmtTime } from './format';
import { Glyph, glyphForKind } from './glyphs';

const W = 320;
const ENERGY_H = 112;
/** The Energy page has more room: a taller plot. */
const ENERGY_H_TALL = 160;
const BAND_GAP = 6;
// Stress is a thin band of level colours directly under the energy plot, on the same time axis.
// It has no scale of its own: the colour is the level.
const BAND_H = 10;
const BINS = 72;
/** Height and smallest width of an activity block in the lane under the chart (a comfortable tap target). */
const LANE_H = 32;
const BLOCK_MIN_W = 32;

const LEVEL_COLOR: Record<StressLevel, string> = {
  rest: 'var(--stress-rest)',
  low: 'var(--stress-low)',
  medium: 'var(--stress-medium)',
  high: 'var(--stress-high)',
  activity: 'var(--stress-activity)',
};

const LEGEND: { level: StressLevel; label: string }[] = [
  { level: 'rest', label: 'Rest' },
  { level: 'low', label: 'Low' },
  { level: 'medium', label: 'Medium' },
  { level: 'high', label: 'High' },
];
/** Shown only on days with such bars: the watch measured no stress because you were moving. */
const ACTIVE_LEGEND: { level: StressLevel; label: string } = { level: 'activity', label: 'Too active to measure' };

interface Bin {
  /** The level most samples of the column have; `null` for a column without any sample. */
  level: StressLevel | null;
}

/** Groups the stress samples into at most BINS columns along the same time axis as the energy line. */
function binStress(stress: StressBlock, t0: number, t1: number): Bin[] {
  const span = Math.max(1, t1 - t0);
  const groups: { levels: Map<StressLevel, number> }[] = Array.from({ length: BINS }, () => ({ levels: new Map() }));
  for (const s of stress.series) {
    const t = Date.parse(s.time);
    if (Number.isNaN(t) || t < t0 || t > t1) continue;
    const g = groups[Math.min(BINS - 1, Math.floor(((t - t0) / span) * BINS))];
    if (!g) continue;
    g.levels.set(s.level, (g.levels.get(s.level) ?? 0) + 1);
  }
  return groups.map((g) => {
    let level: StressLevel | null = null;
    let best = 0;
    for (const [l, c] of g.levels) {
      if (c > best) {
        best = c;
        level = l;
      }
    }
    return { level };
  });
}

/** Neighbouring columns of the same level as one run (`from` inclusive, `to` exclusive); columns without a reading are gaps. */
function bandRuns(bins: Bin[]): { from: number; to: number; level: StressLevel }[] {
  const runs: { from: number; to: number; level: StressLevel }[] = [];
  bins.forEach((b, i) => {
    if (b.level === null) return;
    const last = runs[runs.length - 1];
    if (last && last.to === i && last.level === b.level) last.to = i + 1;
    else runs.push({ from: i, to: i + 1, level: b.level });
  });
  return runs;
}

/** One activity of the day, drawn as a block in the lane under the chart. */
export interface TimelineActivity {
  id: string;
  kind: string;
  /** "Run": what a screen reader hears. */
  name: string;
  start: string;
  duration_s: number | null;
}

interface Props {
  energy: EnergyBlock;
  stress: StressBlock;
  timezone: string;
  /** The night that ended on this day: drawn as a shaded block behind the line and the stress band. */
  sleep?: { start: string; end: string } | null;
  /** The day's activities: labelled blocks in a lane under the time axis, each a link to the activity. */
  activities?: readonly TimelineActivity[];
  /** A taller plot (the Energy page). */
  tall?: boolean;
}

/**
 * The day's energy line with the stress bars under it on the same time axis, and the wake time
 * marked by a dashed line. Everything is drawn from the API's own series.
 */
export function EnergyDayChart({ energy, stress, timezone, sleep = null, activities = [], tall = false }: Props) {
  const points = energy.series
    .map((p) => ({ t: Date.parse(p.time), v: p.value }))
    .filter((p): p is { t: number; v: number } => !Number.isNaN(p.t) && p.v !== null);
  if (points.length < 2) {
    return (
      <p>
        <Unavailable />
      </p>
    );
  }
  const energyH = tall ? ENERGY_H_TALL : ENERGY_H;
  const bandTop = energyH + BAND_GAP;
  const H = bandTop + BAND_H;
  const t0 = points[0]!.t;
  const t1 = points[points.length - 1]!.t;
  const span = Math.max(1, t1 - t0);
  const x = (t: number) => ((t - t0) / span) * W;
  const y = (v: number) => energyH - (Math.min(100, Math.max(0, v)) / 100) * (energyH - 4) - 2;
  // Only energy has a value axis (0 to 100).
  const ticks = plotTicks([0, 50, 100], 'int', (v) => y(v) / H);

  const line = points.map((p) => `${x(p.t).toFixed(1)},${y(p.v).toFixed(1)}`).join(' ');
  const area = `0,${energyH} ${line} ${W},${energyH}`;

  const bins = binStress(stress, t0, t1);
  const slot = W / BINS;
  const anyStress = stress.series.some((s) => s.stress !== null || s.level === 'rest' || s.level === 'activity');

  const wake = energy.wake_time ? Date.parse(energy.wake_time) : NaN;
  const wakeX = !Number.isNaN(wake) && wake >= t0 && wake <= t1 ? x(wake) : null;
  const wakeText = fmtTime(energy.wake_time, timezone);

  // The night, clipped to the time the chart covers.
  const sleepFrom = sleep ? Math.max(t0, Date.parse(sleep.start)) : NaN;
  const sleepTo = sleep ? Math.min(t1, Date.parse(sleep.end)) : NaN;
  const sleepBlock = !Number.isNaN(sleepFrom) && !Number.isNaN(sleepTo) && sleepTo > sleepFrom ? { x: x(sleepFrom), w: x(sleepTo) - x(sleepFrom) } : null;
  const blocks = activities.flatMap((a) => {
    const start = Date.parse(a.start);
    if (Number.isNaN(start) || start > t1) return [];
    const end = start + (a.duration_s ?? 0) * 1000;
    if (end < t0) return [];
    const from = Math.max(t0, start);
    const to = Math.min(t1, Math.max(end, from));
    return [{ a, left: ((from - t0) / span) * 100, width: ((to - from) / span) * 100 }];
  });

  const first = fmtTime(new Date(t0).toISOString(), timezone);
  const mid = fmtTime(new Date(t0 + span / 2).toISOString(), timezone);
  const last = fmtTime(new Date(t1).toISOString(), timezone);

  const summary = [
    `Energy today from ${first} to ${last}: now ${fmtInt(energy.now) ?? 'unavailable'}, high ${fmtInt(energy.high) ?? 'unavailable'}, low ${fmtInt(energy.low) ?? 'unavailable'}.`,
    wakeText ? `You woke at ${wakeText}.` : '',
    sleepBlock ? `Asleep from ${fmtTime(sleep!.start, timezone)} to ${fmtTime(sleep!.end, timezone)}.` : '',
    stress.average === null ? 'Stress is unavailable.' : `Average stress ${fmtInt(stress.average)}${stress.main_level ? `, mostly ${stress.main_level}` : ''}.`,
  ]
    .filter(Boolean)
    .join(' ');

  const xAxis = (
    <PlotXLabels
      labels={[
        { at: 0, text: first ?? '' },
        { at: 0.5, text: mid ?? '' },
        { at: 1, text: last ?? '' },
      ]}
    />
  );

  return (
    <div className="day-chart">
      <PlotFrame ticks={ticks} viewW={W} viewH={H} label={`Energy through the day from 0 to 100, with the stress level of each stretch in a band below${sleepBlock ? ' and the time asleep shaded' : ''}`} xAxis={xAxis}>
        {sleepBlock ? <rect x={sleepBlock.x} y={0} width={sleepBlock.w} height={H} style={{ fill: 'var(--sleep)', fillOpacity: 0.16 }} /> : null}
        <polygon points={area} style={{ fill: 'var(--energy-fill)' }} />
        <polyline points={line} fill="none" strokeWidth={2.5} strokeLinejoin="round" vectorEffect="non-scaling-stroke" style={{ stroke: 'var(--energy)' }} />
        {anyStress
          ? bandRuns(bins).map((r) => <rect key={r.from} x={r.from * slot} y={bandTop} width={(r.to - r.from) * slot + 0.3} height={BAND_H} style={{ fill: LEVEL_COLOR[r.level] }} />)
          : null}
        {wakeX !== null ? <line x1={wakeX} x2={wakeX} y1={0} y2={H} strokeWidth={2} strokeDasharray="4 3" vectorEffect="non-scaling-stroke" style={{ stroke: 'var(--text)' }} /> : null}
      </PlotFrame>
      {blocks.length > 0 ? (
        // The same two columns as the plot above, so a block sits under the time it happened. It is outside the plot's
        // image role because every block is a link.
        <div className="plot plot--bare day-lane">
          <div className="plot__y" aria-hidden="true" style={{ height: LANE_H }}>
            <span className="plot__y-size">100</span>
          </div>
          <ul className="plot__area day-lane__list" style={{ height: LANE_H }} aria-label="Activities of the day">
            {blocks.map(({ a, left, width }) => (
              <li key={a.id} className="day-lane__item" style={{ left: `${(left + width / 2).toFixed(2)}%`, width: `max(${BLOCK_MIN_W}px, ${width.toFixed(2)}%)` }}>
                <Link className="day-lane__block" to={`/activities/${a.id}`} aria-label={`${a.name}, ${fmtTime(a.start, timezone) ?? ''}${a.duration_s ? `, ${Math.round(a.duration_s / 60)} min` : ''}`}>
                  <Glyph name={glyphForKind(a.kind)} size={18} />
                </Link>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      <div className="day-chart__legend">
        {sleepBlock ? (
          <span>
            <i className="day-chart__swatch day-chart__swatch--sleep" aria-hidden="true" />
            Asleep
          </span>
        ) : null}
        {anyStress
          ? [...LEGEND, ...(stress.series.some((s) => s.level === 'activity') ? [ACTIVE_LEGEND] : [])].map((l) => (
              <span key={l.level}>
                <i className="day-chart__swatch" aria-hidden="true" style={{ background: LEVEL_COLOR[l.level] }} />
                {l.label}
              </span>
            ))
          : null}
      </div>
      <p className="sr-only">{summary}</p>
    </div>
  );
}
