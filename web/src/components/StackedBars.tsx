import { tickCountFor, zeroBasedScale } from '../lib/ticks';
import { PlotFrame, plotTicks } from './PlotFrame';
import { EmptyNote } from './Unavailable';

export interface StackPart {
  key: string;
  color: string;
  /** Minutes per bar; null or 0 adds nothing to the stack. */
  values: (number | null)[];
}

interface StackedBarsProps {
  /** Shown instead of the chart when nothing was recorded. */
  emptyText?: string;
  /** Bottom part first. */
  parts: StackPart[];
  /** Label under each bar; empty strings when `axisLabels` is used. */
  labels: string[];
  /** A few labels under the chart instead of one per bar. */
  axisLabels?: string[];
  /** Accessible name of the chart. */
  label: string;
  /** Text summary for screen readers. */
  summary?: string;
  height?: number;
}

const W = 320;
const LABEL_BUDGET_PX = 16;

const num = (v: number | null | undefined): number => (typeof v === 'number' && Number.isFinite(v) && v > 0 ? v : 0);

/** The height of each bar: the sum of its parts. */
export function stackTotals(parts: StackPart[], n: number): number[] {
  return Array.from({ length: n }, (_, i) => parts.reduce((sum, p) => sum + num(p.values[i]), 0));
}

/** Bars made of stacked parts (stress minutes per level per day), on a value axis in hours. */
export function StackedBars({ parts, labels, axisLabels, label, summary, emptyText, height = 110 }: StackedBarsProps) {
  const n = labels.length;
  const totals = stackTotals(parts, n);
  const top = Math.max(0, ...totals);
  if (top <= 0) {
    return (
      <EmptyNote>{emptyText ?? 'Nothing recorded for this period.'}</EmptyNote>
    );
  }
  const scale = zeroBasedScale(top, tickCountFor(height, LABEL_BUDGET_PX), 'minutes');
  const slot = W / n;
  const barW = Math.min(32, slot * 0.7);
  const usable = height - 4;
  const y = (v: number) => height - (v / scale.top) * usable;
  const ticks = plotTicks(scale.ticks, 'minutes', (v) => y(v) / height);
  const xAxis = axisLabels ? (
    <div className="chart__axis" aria-hidden="true">
      {axisLabels.map((t) => (
        <span key={t}>{t}</span>
      ))}
    </div>
  ) : (
    <div className="chart__axis chart__axis--bars" aria-hidden="true" style={{ display: 'grid', gridTemplateColumns: `repeat(${n}, minmax(0, 1fr))`, textAlign: 'center' }}>
      {labels.map((t, i) => (
        <span key={`${t}-${i}`}>{t}</span>
      ))}
    </div>
  );
  return (
    <div className="chart" data-chart="stacked">
      <PlotFrame ticks={ticks} viewW={W} viewH={height} label={label} xAxis={xAxis}>
        {Array.from({ length: n }, (_, i) => {
          const x = i * slot + (slot - barW) / 2;
          if ((totals[i] as number) <= 0) return <rect key={i} x={x} y={height - 4} width={barW} height={4} rx={2} style={{ fill: 'var(--track)' }} />;
          let base = 0;
          return (
            <g key={i}>
              {parts.map((p) => {
                const v = num(p.values[i]);
                if (v <= 0) return null;
                const y1 = y(base + v);
                const h = y(base) - y1;
                base += v;
                return <rect key={p.key} x={x} y={y1} width={barW} height={h} style={{ fill: p.color }} />;
              })}
            </g>
          );
        })}
      </PlotFrame>
      {summary ? <p className="sr-only">{summary}</p> : null}
    </div>
  );
}
