import { PlotFrame, PlotXLabels, plotTicks, Unavailable } from '../../components';
import { fmtShortDay } from '../../lib/dates';
import { tickCountFor, unitTicks } from '../../lib/ticks';
import { toPoints, weightSummary } from './body';
import type { TimedValue } from './types';

const W = 320;
const H = 132;

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
  // Kilograms on the left, whole or half steps as the range needs.
  const ticks = plotTicks(unitTicks(vMin, vMax, Math.min(4, tickCountFor(H, 16)), 'decimal'), 'decimal', y);
  const dayAt = (t: number) => x(t);
  const xAxis = (
    <PlotXLabels
      labels={[
        { at: dayAt(first.t), text: fmtShortDay(first.day) },
        { at: dayAt(midDay.t), text: fmtShortDay(midDay.day) },
        { at: dayAt(last.t), text: fmtShortDay(last.day) },
      ]}
    />
  );

  return (
    <div className="chart weight-chart">
      <PlotFrame
        ticks={ticks}
        viewW={W}
        viewH={H}
        label="Weight trend over 90 days: individual weigh-ins as dots with a smoothed trend line, in kilograms"
        xAxis={xAxis}
        overlay={dots.map((p, i) => (
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
      >
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
      </PlotFrame>
      <p className="sr-only">{summary}</p>
      <p className="stat__sub">
        {lo.toFixed(1)} to {hi.toFixed(1)} kg over {dots.length} weigh-in{dots.length === 1 ? '' : 's'}. Dots are weigh-ins, the line is the smoothed trend.
      </p>
    </div>
  );
}
