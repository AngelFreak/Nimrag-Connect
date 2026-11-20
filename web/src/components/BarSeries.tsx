import { tickCountFor, zeroBasedScale, type AxisUnit } from '../lib/ticks';
import { PlotFrame, plotTicks } from './PlotFrame';
import { Unavailable } from './Unavailable';

interface BarSeriesProps {
  /** One value per bar; null draws an empty stub (no data for that day), never a zero bar. */
  values: (number | null)[];
  /** Label under each bar ("Mon", "Tue"); also used in the accessible summary. */
  labels: string[];
  /** Accessible summary in words: "Steps per day this week: Monday 9,120, ...". */
  label: string;
  color?: string;
  /** Colour for bars below `goal`. */
  belowGoalColor?: string;
  goal?: number;
  /** Index of the bar to draw in `highlightColor` ("now": the latest night, today's steps). */
  highlightIndex?: number;
  /** Colour of the highlighted bar; default the sleep colour. */
  highlightColor?: string;
  /** Text summary of the chart (values per bar, range, average): read by screen readers. */
  summary?: string;
  /** Show the summary as a line under the chart as well (otherwise it is only for screen readers). */
  summaryVisible?: boolean;
  height?: number;
  /** Fixed full-scale value; default is the largest value. */
  max?: number;
  /** A few labels under the chart instead of one per bar (desktop charts label a few dates). */
  axisLabels?: string[];
  /** What the values are, for the value axis on the left (default whole numbers). */
  yUnit?: AxisUnit;
}

const W = 320;
/** Tick labels are sized from --fs-xs; budget for them at 130% text so they never touch. */
const LABEL_BUDGET_PX = 16;

/** Small bar chart (sleep per night, steps per day) drawn as SVG. */
export function BarSeries({ values, labels, label, color = 'var(--stage-deep)', belowGoalColor, goal, highlightIndex, highlightColor = 'var(--sleep)', summary, summaryVisible, height = 84, max, axisLabels, yUnit = 'int' }: BarSeriesProps) {
  const present = values.filter((v): v is number => typeof v === 'number' && Number.isFinite(v));
  if (present.length === 0) {
    return (
      <p>
        <Unavailable />
      </p>
    );
  }
  const rawTop = max ?? Math.max(...present, goal ?? 0);
  const scale = zeroBasedScale(rawTop, tickCountFor(height, LABEL_BUDGET_PX), yUnit);
  const top = max ?? scale.top;
  const slot = W / values.length;
  const barW = Math.min(32, slot * 0.7);
  const usable = height - 4;
  const y = (v: number) => height - (v / top) * usable;
  const ticks = plotTicks(
    scale.ticks.filter((t) => t <= top + 1e-9),
    yUnit,
    (v) => y(v) / height,
  );
  const xAxis = axisLabels ? (
    <div className="chart__axis" aria-hidden="true">
      {axisLabels.map((t) => (
        <span key={t}>{t}</span>
      ))}
    </div>
  ) : (
    <div className="chart__axis" aria-hidden="true" style={{ display: 'grid', gridTemplateColumns: `repeat(${values.length}, 1fr)`, textAlign: 'center' }}>
      {labels.map((t, i) => (
        <span key={`${t}-${i}`}>{t}</span>
      ))}
    </div>
  );
  return (
    <div className="chart">
      <PlotFrame ticks={ticks} viewW={W} viewH={height} label={label} xAxis={xAxis}>
        {goal !== undefined ? <line x1={0} x2={W} y1={y(goal)} y2={y(goal)} style={{ stroke: 'var(--text-2)' }} strokeWidth={1} strokeDasharray="4 4" /> : null}
        {values.map((v, i) => {
          const x = i * slot + (slot - barW) / 2;
          if (typeof v !== 'number' || !Number.isFinite(v)) {
            return <rect key={i} x={x} y={height - 4} width={barW} height={4} rx={2} style={{ fill: 'var(--track)' }} />;
          }
          const fill = i === highlightIndex ? highlightColor : goal !== undefined && v < goal && belowGoalColor ? belowGoalColor : color;
          return <rect key={i} x={x} y={y(v)} width={barW} height={height - y(v)} rx={4} style={{ fill }} />;
        })}
      </PlotFrame>
      {summary ? <p className={summaryVisible ? 'chart-summary' : 'sr-only'}>{summary}</p> : null}
    </div>
  );
}
