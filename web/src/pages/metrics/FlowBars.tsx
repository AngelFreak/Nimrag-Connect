import { PlotFrame, plotTicks, Unavailable } from '../../components';
import type { FlowChart } from './energyDay';

const W = 320;

/** The smallest of 10, 20, 25, 50, 100, ... that is not under `value`: the half height of the chart. */
export function flowTop(value: number): number {
  if (!(value > 0)) return 10;
  const steps = [10, 20, 25, 50, 75, 100, 150, 200];
  return steps.find((s) => s >= value) ?? Math.ceil(value / 50) * 50;
}

/**
 * Energy charged (up, from the zero line) and drained (down) for each day or week. A day without a
 * reading has neither bar. Colours are the energy and warning tokens; the sign is in the axis labels
 * and in the text summary, so colour is never the only cue.
 */
export function FlowBars({ chart, height }: { chart: FlowChart; height: number }) {
  if (!chart.hasData) {
    return (
      <p>
        <Unavailable />
      </p>
    );
  }
  const n = chart.charged.length;
  const top = flowTop(Math.max(...chart.charged.map((v) => v ?? 0), ...chart.drained.map((v) => v ?? 0)));
  const mid = height / 2;
  const usable = mid - 4;
  const slot = W / n;
  const barW = Math.min(18, slot * 0.62);
  const h = (v: number) => (v / top) * usable;
  const ticks = plotTicks([top, 0, -top], 'int', (v) => (mid - (v / top) * usable) / height).map((t, i) => ({ ...t, label: i === 0 ? `+${top}` : i === 2 ? `−${top}` : '0' }));
  const xAxis = chart.axisLabels ? (
    <div className="chart__axis" aria-hidden="true">
      {chart.axisLabels.map((t) => (
        <span key={t}>{t}</span>
      ))}
    </div>
  ) : (
    <div className="chart__axis chart__axis--bars" aria-hidden="true" style={{ display: 'grid', gridTemplateColumns: `repeat(${n}, minmax(0, 1fr))`, textAlign: 'center' }}>
      {chart.labels.map((t, i) => (
        <span key={`${t}-${i}`}>{t}</span>
      ))}
    </div>
  );
  return (
    <div className="chart">
      <PlotFrame ticks={ticks} viewW={W} viewH={height} label="Energy charged and drained per day" xAxis={xAxis}>
        {chart.charged.map((v, i) => {
          const x = i * slot + (slot - barW) / 2;
          return v === null || v <= 0 ? null : <rect key={`c${i}`} x={x} y={mid - h(v)} width={barW} height={h(v)} rx={3} style={{ fill: 'var(--energy)' }} />;
        })}
        {chart.drained.map((v, i) => {
          const x = i * slot + (slot - barW) / 2;
          return v === null || v <= 0 ? null : <rect key={`d${i}`} x={x} y={mid} width={barW} height={h(v)} rx={3} style={{ fill: 'var(--warn)' }} />;
        })}
        <line x1={0} x2={W} y1={mid} y2={mid} strokeWidth={1.5} vectorEffect="non-scaling-stroke" style={{ stroke: 'var(--text-2)' }} />
      </PlotFrame>
      <p className="sr-only">{chart.summary}</p>
    </div>
  );
}
