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

/** A marker drawn on one point: green dot, orange square, red triangle (HRV status). */
export interface ChartMarker {
  shape: 'dot' | 'square' | 'triangle';
  /** Canvas colour (a hex value; CSS variables do not work on a canvas). */
  color: string;
}

export interface ChartSeries {
  label: string;
  color: ChartColor;
  /** One value per x; null leaves a gap (never drawn as 0). */
  data: (number | null)[];
  /** Fill the area under the line (the Day view energy curve). */
  area?: boolean;
  width?: number;
  /** Draw a dot on every point, so a lone reading is visible (a line needs two). */
  points?: boolean;
  /** Draw this marker on the points that have one (same length as `data`; null draws none). */
  markers?: (ChartMarker | null)[];
  /** Draw no line between the points (markers only). */
  noLine?: boolean;
}

/** A range that changes over time: lower and upper edge per x (null where unknown). */
export interface BandSeries {
  lo: (number | null)[];
  hi: (number | null)[];
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
  /**
   * Shaded range behind the lines (HRV: your usual range): one fixed `[low, high]`, or a range
   * per x (`BandSeries`, same length as `x`) drawn as a ribbon. A lone point of a ribbon is a
   * rectangle one x unit wide.
   */
  band?: [number, number] | BandSeries;
  /** Fixed y range; by default fitted to the data with some padding. */
  yRange?: [number, number];
  /** Fixed x range ([0, 24] hours for a day chart); by default the first and last x value. */
  xRange?: [number, number];
  /** Show the summary as a line under the chart too (otherwise only screen readers get it). */
  summaryVisible?: boolean;
  /**
   * Axis labels and grid lines (desktop charts): true for both axes, 'y' for the value axis only
   * (put your own date labels under the chart), false for the bare phone style.
   */
  axes?: boolean | 'y';
}

function bandExtent(band: [number, number] | BandSeries): [number, number] {
  if (Array.isArray(band)) return band;
  const vals = [...band.lo, ...band.hi].filter((v): v is number => typeof v === 'number' && Number.isFinite(v));
  return vals.length ? [Math.min(...vals), Math.max(...vals)] : [Infinity, -Infinity];
}

function drawBand(u: uPlot, band: [number, number] | BandSeries, x: number[]) {
  const ctx = u.ctx;
  ctx.save();
  ctx.fillStyle = BAND;
  if (Array.isArray(band)) {
    const top = u.valToPos(band[1], 'y', true);
    const bottom = u.valToPos(band[0], 'y', true);
    ctx.fillRect(u.bbox.left, top, u.bbox.width, bottom - top);
  } else {
    // Runs of consecutive x with a range become one ribbon each.
    let run: number[] = [];
    const flush = () => {
      if (run.length === 0) return;
      const px = (i: number) => u.valToPos(x[i] as number, 'x', true);
      const py = (v: number | null) => u.valToPos(v as number, 'y', true);
      ctx.beginPath();
      if (run.length === 1) {
        const i = run[0] as number;
        const half = Math.abs(u.valToPos(1, 'x', true) - u.valToPos(0, 'x', true)) / 2;
        ctx.rect(px(i) - half, py(band.hi[i] ?? null), half * 2, py(band.lo[i] ?? null) - py(band.hi[i] ?? null));
      } else {
        run.forEach((i, k) => (k === 0 ? ctx.moveTo(px(i), py(band.hi[i] ?? null)) : ctx.lineTo(px(i), py(band.hi[i] ?? null))));
        [...run].reverse().forEach((i) => ctx.lineTo(px(i), py(band.lo[i] ?? null)));
        ctx.closePath();
      }
      ctx.fill();
      run = [];
    };
    x.forEach((_, i) => {
      if (band.lo[i] != null && band.hi[i] != null) run.push(i);
      else flush();
    });
    flush();
  }
  ctx.restore();
}

function drawMarkers(u: uPlot, x: number[], series: ChartSeries[]) {
  const ctx = u.ctx;
  const r = 5 * uPlot.pxRatio;
  ctx.save();
  for (const s of series) {
    s.markers?.forEach((m, i) => {
      const v = s.data[i];
      if (!m || v == null) return;
      const cx = u.valToPos(x[i] as number, 'x', true);
      const cy = u.valToPos(v, 'y', true);
      ctx.fillStyle = m.color;
      ctx.beginPath();
      if (m.shape === 'dot') ctx.arc(cx, cy, r, 0, Math.PI * 2);
      else if (m.shape === 'square') ctx.rect(cx - r * 0.9, cy - r * 0.9, r * 1.8, r * 1.8);
      else {
        ctx.moveTo(cx, cy - r * 1.1);
        ctx.lineTo(cx + r * 1.1, cy + r * 0.9);
        ctx.lineTo(cx - r * 1.1, cy + r * 0.9);
        ctx.closePath();
      }
      ctx.fill();
    });
  }
  ctx.restore();
}

function hasData(series: ChartSeries[]): boolean {
  return series.some((s) => s.data.some((v) => typeof v === 'number' && Number.isFinite(v)));
}

/** Line chart on uPlot with an accessible name and a text summary. */
export function SeriesChart({ label, summary, x, series, height = 90, time = false, band, yRange, xRange, summaryVisible = false, axes = false }: SeriesChartProps) {
  const host = useRef<HTMLDivElement>(null);
  const available = hasData(series) && x.length > 0;
  const key = useMemo(() => JSON.stringify([x, series, height, time, band, yRange, xRange, axes]), [x, series, height, time, band, yRange, xRange, axes]);

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
        x: xRange ? { time, range: () => xRange } : { time },
        y: {
          range: (_u, min, max) => {
            if (yRange) return yRange;
            let lo = min;
            let hi = max;
            if (band) {
              const [bl, bh] = bandExtent(band);
              lo = Math.min(lo, bl);
              hi = Math.max(hi, bh);
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
            points: s.points ? { show: true, size: 7, width: 0, fill: stroke } : { show: false },
            spanGaps: false,
            ...(s.noLine ? { paths: () => null } : {}),
            ...(s.area ? { fill: s.color === 'energy' ? ENERGY_FILL : `${stroke}33` } : {}),
          };
        }),
      ],
      hooks: {
        ...(band ? { drawClear: [(u: uPlot) => drawBand(u, band, x)] } : {}),
        ...(series.some((s) => s.markers) ? { draw: [(u: uPlot) => drawMarkers(u, x, series)] } : {}),
      },
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
      <>
        <p>
          <Unavailable />
        </p>
        {summaryVisible ? <p className="chart-summary">{summary}</p> : null}
      </>
    );
  }

  return (
    <div className="series-chart">
      <div ref={host} role="img" aria-label={label} style={{ minHeight: axes === true ? height + 28 : height }} />
      <p className={summaryVisible ? 'chart-summary' : 'sr-only'}>{summary}</p>
    </div>
  );
}
