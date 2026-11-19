import { Unavailable } from '../../components';
import { fmtShortDay } from '../../lib/dates';
import { toPoints, weightSummary } from './body';
import type { TimedValue } from './types';

const W = 320;
const H = 120;

/**
 * Weight over 90 days: each weigh-in is a dot, the smoothed trend a line. Dots are HTML so they
 * stay round when the chart stretches; the lines are SVG with a constant stroke width.
 * Dot versus line carries the difference, not colour alone.
 */
export function WeightChart({ weights, trend }: { weights: TimedValue[]; trend: TimedValue[] }) {
  const dots = toPoints(weights);
  const line = toPoints(trend);
  if (dots.length === 0) {
    return (
      <p>
        <Unavailable />
      </p>
    );
  }
  const all = [...dots, ...line];
  const t0 = Math.min(...all.map((p) => p.t));
  const t1 = Math.max(...all.map((p) => p.t));
  const lo = Math.min(...all.map((p) => p.v));
  const hi = Math.max(...all.map((p) => p.v));
  const pad = (hi - lo) * 0.15 || 0.5;
  const vMin = lo - pad;
  const vMax = hi + pad;
  const x = (t: number) => (t1 === t0 ? 0.5 : (t - t0) / (t1 - t0));
  const y = (v: number) => 1 - (v - vMin) / (vMax - vMin);
  const first = dots[0]!;
  const last = dots[dots.length - 1]!;
  const midDay = dots[Math.floor((dots.length - 1) / 2)]!;
  const summary = weightSummary(dots, line);

  return (
    <div className="chart">
      <div className="weight-chart" role="img" aria-label="Weight trend over 90 days: individual weigh-ins as dots with a smoothed trend line" style={{ position: 'relative', height: H }}>
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true" style={{ position: 'absolute', inset: 0, width: '100%', height: H }}>
          <path d={`M0 ${H * 0.25}H${W}M0 ${H * 0.5}H${W}M0 ${H * 0.75}H${W}`} style={{ stroke: 'var(--line)' }} strokeWidth={1} vectorEffect="non-scaling-stroke" />
          {line.length > 1 ? (
            <polyline
              fill="none"
              style={{ stroke: 'var(--link)' }}
              strokeWidth={2.5}
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
              points={line.map((p) => `${(x(p.t) * W).toFixed(1)},${(y(p.v) * H).toFixed(1)}`).join(' ')}
            />
          ) : null}
        </svg>
        {dots.map((p, i) => (
          <span
            key={`${p.t}-${i}`}
            aria-hidden="true"
            style={{
              position: 'absolute',
              left: `${x(p.t) * 100}%`,
              top: `${y(p.v) * 100}%`,
              width: 7,
              height: 7,
              borderRadius: '50%',
              background: 'var(--link-hover)',
              transform: 'translate(-50%, -50%)',
            }}
          />
        ))}
      </div>
      <div className="chart__axis" aria-hidden="true">
        <span>{fmtShortDay(first.day)}</span>
        <span>{fmtShortDay(midDay.day)}</span>
        <span>{fmtShortDay(last.day)}</span>
      </div>
      <p className="sr-only">{summary}</p>
      <p className="stat__sub">
        {lo.toFixed(1)} to {hi.toFixed(1)} kg over {dots.length} weigh-in{dots.length === 1 ? '' : 's'}. Dots are weigh-ins, the line is the smoothed trend.
      </p>
    </div>
  );
}
