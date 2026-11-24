import type { ReactNode } from 'react';
import { formatTicks, type AxisUnit } from '../lib/ticks';

/** One value label on the left of a plot, with its grid line. */
export interface PlotTick {
  label: string;
  /** Where it sits, 0 at the top of the plot and 1 at the bottom. */
  at: number;
}

/** A label under the plot, at a share of its width (0 left edge, 1 right edge). */
export interface PlotXLabel {
  at: number;
  text: string;
}

interface PlotFrameProps {
  /** Value labels on the left. Each also gets a faint grid line across the plot. */
  ticks: PlotTick[];
  /** Plot width in the SVG's own units (the SVG stretches to the available width). */
  viewW: number;
  /** Plot height in the SVG's own units; the SVG is drawn this many CSS px tall. */
  viewH: number;
  /** Accessible name of the plot (the SVG and HTML inside it are hidden from screen readers). */
  label: string;
  /** SVG content drawn over the grid lines. */
  children: ReactNode;
  /** HTML drawn on top of the SVG (round dots that must not stretch). Positioned in plot percentages. */
  overlay?: ReactNode;
  /** Row under the plot, in the plot's column so it lines up with it. A `PlotXLabels`, or your own. */
  xAxis?: ReactNode;
  /** Drawn behind the grid lines (shaded bands). */
  behind?: ReactNode;
  /** A second value axis on the right, for a line on its own scale. Labels only, no grid lines. */
  rightTicks?: PlotTick[];
}

/**
 * Plot area of an SVG chart with a value axis, like Garmin Connect's: value labels on the left, a
 * faint grid line at each, and the plot beside them. The labels are HTML so they are not distorted
 * by `preserveAspectRatio="none"` and follow the text size; the gutter is as wide as the widest
 * label, and the x labels sit in the plot's column, so they line up with it.
 */
export function PlotFrame({ ticks, viewW, viewH, label, children, overlay, xAxis, behind, rightTicks }: PlotFrameProps) {
  const widest = ticks.reduce((a, t) => (t.label.length > a.length ? t.label : a), '');
  return (
    <div className={['plot', !xAxis && 'plot--bare', rightTicks && 'plot--right'].filter(Boolean).join(' ')} role="img" aria-label={label}>
      <div className="plot__y" aria-hidden="true" style={{ height: viewH }}>
        <span className="plot__y-size">{widest}</span>
        {ticks.map((t) => (
          <span key={`${t.label}-${t.at}`} className="plot__y-tick" style={{ top: `${t.at * 100}%` }}>
            {t.label}
          </span>
        ))}
      </div>
      <div className="plot__area" style={{ height: viewH }}>
        <svg viewBox={`0 0 ${viewW} ${viewH}`} preserveAspectRatio="none" aria-hidden="true" focusable="false">
          {behind}
          {ticks.map((t) => (
            <line key={`${t.label}-${t.at}`} x1={0} x2={viewW} y1={t.at * viewH} y2={t.at * viewH} strokeWidth={1} vectorEffect="non-scaling-stroke" style={{ stroke: 'var(--line)' }} />
          ))}
          {children}
        </svg>
        {overlay}
      </div>
      {rightTicks ? (
        <div className="plot__y plot__y--right" aria-hidden="true" style={{ height: viewH }}>
          <span className="plot__y-size">{rightTicks.reduce((a, t) => (t.label.length > a.length ? t.label : a), '')}</span>
          {rightTicks.map((t) => (
            <span key={`${t.label}-${t.at}`} className="plot__y-tick" style={{ top: `${t.at * 100}%` }}>
              {t.label}
            </span>
          ))}
        </div>
      ) : null}
      {xAxis ? <div className="plot__x">{xAxis}</div> : null}
    </div>
  );
}

/**
 * Labels under a plot, each at its share of the width: the first flush left, the last flush right,
 * the ones between centred on their position (the same rule as the activity time axis).
 */
export function PlotXLabels({ labels }: { labels: PlotXLabel[] }) {
  return (
    <div className="time-axis" aria-hidden="true">
      {labels.map((l, i) => (
        <span key={`${l.text}-${i}`} style={{ left: `${l.at * 100}%`, transform: `translateX(-${l.at * 100}%)` }}>
          {l.text}
        </span>
      ))}
    </div>
  );
}

/** `n` labels spread evenly from the left edge to the right edge. */
export function evenLabels(texts: string[]): PlotXLabel[] {
  return texts.map((text, i) => ({ text, at: texts.length < 2 ? 0 : i / (texts.length - 1) }));
}

/** Value labels for these tick values, each placed by `at` (value to 0..1 from the top). */
export function plotTicks(values: number[], unit: AxisUnit, at: (v: number) => number): PlotTick[] {
  const labels = formatTicks(values, unit);
  return values.map((v, i) => ({ label: labels[i] ?? '', at: at(v) }));
}
