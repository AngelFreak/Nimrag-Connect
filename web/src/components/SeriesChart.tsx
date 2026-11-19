import { useEffect, useMemo, useRef } from 'react';
import uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';
import { Unavailable } from './Unavailable';

// uPlot draws on a canvas, so it needs real colour values rather than CSS variables.
// They mirror styles/tokens.css.
export const CHART_COLORS = {
  link: '#7dbbff',
  linkSoft: '#a9d2ff',
  heart: '#ff6b6b',
  sleep: '#a594ff',
  energy: '#4fd1b5',
  steps: '#f2d45c',
  warn: '#ffb066',
} as const;
export type ChartColor = keyof typeof CHART_COLORS;

const TEXT_2 = '#9aa7ac';
const LINE = '#243037';
const BAND = '#1d3144';
const ENERGY_FILL = '#173a36';

export interface ChartSeries {
  label: string;
  color: ChartColor;
  /** One value per x; null leaves a gap (never drawn as 0). */
  data: (number | null)[];
  /** Fill the area under the line (the Day view energy curve). */
  area?: boolean;
  width?: number;
}

interface SeriesChartProps {
  /** Short accessible name: "Overnight HRV over 4 weeks". */
  label: string;
  /** Text summary of what the chart shows, read by screen readers and kept in the page text. */
  summary: string;
  /** X values: unix seconds when `time` is set, otherwise plain numbers (0..n). */
  x: number[];
  series: ChartSeries[];
  height?: number;
  time?: boolean;
  /** Shaded range behind the lines (HRV: your usual range). */
  band?: [number, number];
  /** Fixed y range; by default fitted to the data with some padding. */
  yRange?: [number, number];
  /**
   * Axis labels and grid lines (desktop charts): true for both axes, 'y' for the value axis only
   * (put your own date labels under the chart), false for the bare phone style.
   */
  axes?: boolean | 'y';
}

function hasData(series: ChartSeries[]): boolean {
  return series.some((s) => s.data.some((v) => typeof v === 'number' && Number.isFinite(v)));
}

/** Line chart on uPlot with an accessible name and a text summary. */
export function SeriesChart({ label, summary, x, series, height = 90, time = false, band, yRange, axes = false }: SeriesChartProps) {
  const host = useRef<HTMLDivElement>(null);
  const available = hasData(series) && x.length > 0;
  const key = useMemo(() => JSON.stringify([x, series, height, time, band, yRange, axes]), [x, series, height, time, band, yRange, axes]);

  useEffect(() => {
    const el = host.current;
    if (!el || !available) return;
    const width = Math.max(120, el.clientWidth || 320);

    const opts: uPlot.Options = {
      width,
      height,
      padding: [6, 6, axes === true ? 0 : 6, axes ? 0 : 6],
      legend: { show: false },
      cursor: { show: false },
      select: { show: false, left: 0, top: 0, width: 0, height: 0 },
      scales: {
        x: { time },
        y: {
          range: (_u, min, max) => {
            if (yRange) return yRange;
            let lo = min;
            let hi = max;
            if (band) {
              lo = Math.min(lo, band[0]);
              hi = Math.max(hi, band[1]);
            }
            const pad = (hi - lo) * 0.12 || 1;
            return [lo - pad, hi + pad];
          },
        },
      },
      axes: [
        { show: axes === true, stroke: TEXT_2, grid: { stroke: LINE, width: 1 }, ticks: { show: false }, size: 28, font: '12px Inter, sans-serif' },
        { show: axes !== false, stroke: TEXT_2, grid: { stroke: LINE, width: 1 }, ticks: { show: false }, size: 40, font: '12px Inter, sans-serif' },
      ],
      series: [
        {},
        ...series.map((s): uPlot.Series => {
          const stroke = CHART_COLORS[s.color];
          return {
            label: s.label,
            stroke,
            width: s.width ?? 2.5,
            points: { show: false },
            spanGaps: false,
            ...(s.area ? { fill: s.color === 'energy' ? ENERGY_FILL : `${stroke}33` } : {}),
          };
        }),
      ],
      hooks: band
        ? {
            drawClear: [
              (u) => {
                const top = u.valToPos(band[1], 'y', true);
                const bottom = u.valToPos(band[0], 'y', true);
                u.ctx.save();
                u.ctx.fillStyle = BAND;
                u.ctx.fillRect(u.bbox.left, top, u.bbox.width, bottom - top);
                u.ctx.restore();
              },
            ],
          }
        : {},
    };

    const plot = new uPlot(opts, [x, ...series.map((s) => s.data)] as uPlot.AlignedData, el);
    const observer = new ResizeObserver(() => {
      const w = Math.max(120, el.clientWidth);
      if (w !== plot.width) plot.setSize({ width: w, height });
    });
    observer.observe(el);
    return () => {
      observer.disconnect();
      plot.destroy();
    };
    // `key` captures every input that changes the plot.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, available]);

  if (!available) {
    return (
      <p>
        <Unavailable />
      </p>
    );
  }

  return (
    <div className="series-chart">
      <div ref={host} role="img" aria-label={label} style={{ minHeight: axes === true ? height + 28 : height }} />
      <p className="sr-only">{summary}</p>
    </div>
  );
}
