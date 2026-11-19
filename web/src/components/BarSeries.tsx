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
  /** Colour of the last bar when it is "now" (sleep: latest night). */
  highlightLast?: string;
  height?: number;
  /** Fixed full-scale value; default is the largest value. */
  max?: number;
  /** Hide the label row (desktop charts label a few dates instead). */
  axisLabels?: string[];
}

const W = 320;

/** Small bar chart (sleep per night, steps per day) drawn as SVG. */
export function BarSeries({ values, labels, label, color = 'var(--stage-deep)', belowGoalColor, goal, highlightLast, height = 84, max, axisLabels }: BarSeriesProps) {
  const present = values.filter((v): v is number => typeof v === 'number' && Number.isFinite(v));
  if (present.length === 0) {
    return (
      <p>
        <Unavailable />
      </p>
    );
  }
  const top = max ?? Math.max(...present, goal ?? 0);
  const slot = W / values.length;
  const barW = Math.min(32, slot * 0.7);
  const usable = height - 4;
  const y = (v: number) => height - (v / top) * usable;
  return (
    <div className="chart">
      <svg viewBox={`0 0 ${W} ${height}`} preserveAspectRatio="none" role="img" aria-label={label} style={{ height }}>
        {goal !== undefined ? <line x1={0} x2={W} y1={y(goal)} y2={y(goal)} style={{ stroke: 'var(--text-2)' }} strokeWidth={1} strokeDasharray="4 4" /> : null}
        {values.map((v, i) => {
          const x = i * slot + (slot - barW) / 2;
          if (typeof v !== 'number' || !Number.isFinite(v)) {
            return <rect key={i} x={x} y={height - 4} width={barW} height={4} rx={2} style={{ fill: 'var(--track)' }} />;
          }
          const fill = i === values.length - 1 && highlightLast ? highlightLast : goal !== undefined && v < goal && belowGoalColor ? belowGoalColor : color;
          return <rect key={i} x={x} y={y(v)} width={barW} height={height - y(v)} rx={4} style={{ fill }} />;
        })}
      </svg>
      {axisLabels ? (
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
      )}
    </div>
  );
}
