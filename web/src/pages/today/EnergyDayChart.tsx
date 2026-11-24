import type { EnergyBlock } from '../../api/generated/EnergyBlock';
import type { StressBlock } from '../../api/generated/StressBlock';
import type { StressLevel } from '../../api/generated/StressLevel';
import { PlotFrame, PlotXLabels, plotTicks, Unavailable } from '../../components';
import { fmtInt } from '../../lib/format';
import { fmtTime } from './format';

const W = 320;
const ENERGY_H = 112;
// Stress is a thin band of level colours directly under the energy plot, on the same time axis.
// It has no scale of its own: the colour is the level.
const BAND_TOP = ENERGY_H + 6;
const BAND_H = 10;
const H = BAND_TOP + BAND_H;
const BINS = 72;

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

interface Props {
  energy: EnergyBlock;
  stress: StressBlock;
  timezone: string;
}

/**
 * The day's energy line with the stress bars under it on the same time axis, and the wake time
 * marked by a dashed line. Everything is drawn from the API's own series.
 */
export function EnergyDayChart({ energy, stress, timezone }: Props) {
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
  const t0 = points[0]!.t;
  const t1 = points[points.length - 1]!.t;
  const span = Math.max(1, t1 - t0);
  const x = (t: number) => ((t - t0) / span) * W;
  const y = (v: number) => ENERGY_H - (Math.min(100, Math.max(0, v)) / 100) * (ENERGY_H - 4) - 2;
  // Only energy has a value axis (0 to 100).
  const ticks = plotTicks([0, 50, 100], 'int', (v) => y(v) / H);

  const line = points.map((p) => `${x(p.t).toFixed(1)},${y(p.v).toFixed(1)}`).join(' ');
  const area = `0,${ENERGY_H} ${line} ${W},${ENERGY_H}`;

  const bins = binStress(stress, t0, t1);
  const slot = W / BINS;
  const anyStress = stress.series.some((s) => s.stress !== null || s.level === 'rest' || s.level === 'activity');

  const wake = energy.wake_time ? Date.parse(energy.wake_time) : NaN;
  const wakeX = !Number.isNaN(wake) && wake >= t0 && wake <= t1 ? x(wake) : null;
  const wakeText = fmtTime(energy.wake_time, timezone);

  const first = fmtTime(new Date(t0).toISOString(), timezone);
  const mid = fmtTime(new Date(t0 + span / 2).toISOString(), timezone);
  const last = fmtTime(new Date(t1).toISOString(), timezone);

  const summary = [
    `Energy today from ${first} to ${last}: now ${fmtInt(energy.now) ?? 'unavailable'}, high ${fmtInt(energy.high) ?? 'unavailable'}, low ${fmtInt(energy.low) ?? 'unavailable'}.`,
    wakeText ? `You woke at ${wakeText}.` : '',
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
      <PlotFrame ticks={ticks} viewW={W} viewH={H} label="Energy through the day from 0 to 100, with the stress level of each stretch in a band below" xAxis={xAxis}>
        <polygon points={area} style={{ fill: 'var(--energy-fill)' }} />
        <polyline points={line} fill="none" strokeWidth={2.5} strokeLinejoin="round" vectorEffect="non-scaling-stroke" style={{ stroke: 'var(--energy)' }} />
        {anyStress
          ? bandRuns(bins).map((r) => <rect key={r.from} x={r.from * slot} y={BAND_TOP} width={(r.to - r.from) * slot + 0.3} height={BAND_H} style={{ fill: LEVEL_COLOR[r.level] }} />)
          : null}
        {wakeX !== null ? <line x1={wakeX} x2={wakeX} y1={0} y2={H} strokeWidth={2} strokeDasharray="4 3" vectorEffect="non-scaling-stroke" style={{ stroke: 'var(--text)' }} /> : null}
      </PlotFrame>
      <div className="day-chart__legend">
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
