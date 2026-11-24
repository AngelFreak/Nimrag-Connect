import { useId } from 'react';

// Small, quiet charts for the Home cards. Plain SVG and flex boxes: no axes drawn, labels as text
// under the chart. Each one is an image with a text summary for screen readers.

interface SparklineProps {
  /** One value per step along the x axis; null leaves a gap. */
  values: (number | null)[];
  color: string;
  label: string;
  height?: number;
  /** Shaded band behind the line (HRV's usual range). */
  band?: [number, number];
  /** Fill under the line. */
  area?: boolean;
  /** Draw the last point as a dot. */
  dot?: boolean;
  /** A faint dashed line across the chart at this value (the resting heart rate). */
  reference?: number;
}

const W = 200;

/** A thin line over the width of its box. */
export function Sparkline({ values, color, label, height = 56, band, area = false, dot = true, reference }: SparklineProps) {
  const gradient = useId();
  const known = values.filter((v): v is number => v !== null && Number.isFinite(v));
  if (known.length === 0) return null;
  const extra = [...(band ? band : []), ...(reference !== undefined ? [reference] : [])];
  let lo = Math.min(...known, ...extra);
  let hi = Math.max(...known, ...extra);
  if (hi - lo < 1e-9) {
    lo -= 1;
    hi += 1;
  }
  const pad = 4;
  const x = (i: number) => (values.length <= 1 ? W / 2 : (i / (values.length - 1)) * W);
  const y = (v: number) => height - pad - ((v - lo) / (hi - lo)) * (height - pad * 2);
  // Segments between gaps.
  const runs: { x: number; y: number }[][] = [];
  let run: { x: number; y: number }[] = [];
  values.forEach((v, i) => {
    if (v === null || !Number.isFinite(v)) {
      if (run.length) runs.push(run);
      run = [];
    } else {
      run.push({ x: x(i), y: y(v) });
    }
  });
  if (run.length) runs.push(run);
  const last = [...runs].reverse().find((r) => r.length > 0)?.at(-1);
  return (
    <svg className="hchart" viewBox={`0 0 ${W} ${height}`} preserveAspectRatio="none" style={{ height }} role="img" aria-label={label}>
      {band ? <rect x={0} width={W} y={y(band[1])} height={Math.max(1, y(band[0]) - y(band[1]))} style={{ fill: 'var(--band)' }} /> : null}
      {reference !== undefined ? <line x1={0} x2={W} y1={y(reference)} y2={y(reference)} strokeDasharray="3 3" strokeWidth={1} vectorEffect="non-scaling-stroke" style={{ stroke: color, opacity: 0.45 }} /> : null}
      {area ? (
        <defs>
          <linearGradient id={gradient} x1="0" x2="0" y1="0" y2="1">
            <stop offset="0" style={{ stopColor: color, stopOpacity: 0.35 }} />
            <stop offset="1" style={{ stopColor: color, stopOpacity: 0.04 }} />
          </linearGradient>
        </defs>
      ) : null}
      {runs.map((r, i) => {
        const line = r.map((p, j) => `${j === 0 ? 'M' : 'L'}${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(' ');
        return (
          <g key={i}>
            {area && r.length > 1 ? <path d={`${line} L${r.at(-1)!.x.toFixed(1)} ${height} L${r[0]!.x.toFixed(1)} ${height} Z`} fill={`url(#${gradient})`} /> : null}
            <path d={line} fill="none" style={{ stroke: color }} strokeWidth={1.75} strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
          </g>
        );
      })}
      {dot && last ? <circle cx={last.x} cy={last.y} r={2.5} style={{ fill: color }} vectorEffect="non-scaling-stroke" /> : null}
    </svg>
  );
}

interface AreaProps {
  points: { t: number; v: number }[];
  t0: number;
  t1: number;
  color: string;
  label: string;
  height?: number;
  /** Fixed full scale; default 100. */
  max?: number;
}

/** The energy line of one day as a filled area over time. */
export function DayArea({ points, t0, t1, color, label, height = 120, max = 100 }: AreaProps) {
  const gradient = useId();
  if (points.length === 0) return null;
  const span = Math.max(1, t1 - t0);
  const x = (t: number) => ((t - t0) / span) * W;
  const y = (v: number) => height - (Math.min(max, Math.max(0, v)) / max) * (height - 4);
  const line = points.map((p, i) => `${i === 0 ? 'M' : 'L'}${x(p.t).toFixed(1)} ${y(p.v).toFixed(1)}`).join(' ');
  return (
    <svg className="hchart" viewBox={`0 0 ${W} ${height}`} preserveAspectRatio="none" style={{ height }} role="img" aria-label={label}>
      <defs>
        <linearGradient id={gradient} x1="0" x2="0" y1="0" y2="1">
          <stop offset="0" style={{ stopColor: color, stopOpacity: 0.55 }} />
          <stop offset="1" style={{ stopColor: color, stopOpacity: 0.08 }} />
        </linearGradient>
      </defs>
      {[0.25, 0.5, 0.75].map((f) => (
        <line key={f} x1={0} x2={W} y1={height - f * (height - 4)} y2={height - f * (height - 4)} style={{ stroke: 'var(--line)' }} strokeWidth={1} vectorEffect="non-scaling-stroke" />
      ))}
      <path d={`${line} L${x(points.at(-1)!.t).toFixed(1)} ${height} L${x(points[0]!.t).toFixed(1)} ${height} Z`} fill={`url(#${gradient})`} />
      <path d={line} fill="none" style={{ stroke: color }} strokeWidth={1.75} strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

interface BarsProps {
  /** One value per day; null or 0 gives a short grey stub. */
  values: (number | null)[];
  /** One letter under each bar. */
  letters: string[];
  color: string;
  label: string;
  /** Index of the bar to draw darker ("today"). */
  highlight?: number;
  /** Fixed full scale; default the largest value. */
  max?: number;
  height?: number;
}

/** Rounded bars, one per day, with a letter under each. */
export function MiniBars({ values, letters, color, label, highlight, max, height = 120 }: BarsProps) {
  const present = values.filter((v): v is number => v !== null && Number.isFinite(v) && v > 0);
  const top = max ?? (present.length ? Math.max(...present) : 1);
  return (
    <div className="hbars" role="img" aria-label={label}>
      <div className="hbars__plot" style={{ height }}>
        {values.map((v, i) => {
          const has = v !== null && Number.isFinite(v) && v > 0;
          return (
            <span className="hbars__slot" key={i}>
              <span
                className={has ? 'hbars__bar' : 'hbars__bar hbars__bar--empty'}
                style={has ? { height: `${Math.max(4, (v / top) * 100)}%`, background: color, opacity: highlight === undefined || highlight === i ? 1 : 0.55 } : undefined}
              />
            </span>
          );
        })}
      </div>
      <div className="hbars__letters" aria-hidden="true">
        {letters.map((l, i) => (
          <span key={i}>{l}</span>
        ))}
      </div>
    </div>
  );
}

/** A strip of small dots, one per day: filled on the days with something (the "Last 4 weeks" footer). */
export function DotStrip({ days, label }: { days: boolean[]; label: string }) {
  return (
    <div className="hdots" role="img" aria-label={label}>
      {days.map((on, i) => (
        <span key={i} className={on ? 'hdots__dot hdots__dot--on' : 'hdots__dot'} />
      ))}
    </div>
  );
}
