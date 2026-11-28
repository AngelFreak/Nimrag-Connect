import { useEffect, useMemo, useRef } from 'react';
import uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';
import { extremeTicks, formatTicks, roundTick, tickCountFor, unitTicks, type AxisUnit } from '../lib/ticks';
import { useTheme } from '../lib/theme';
import type { CursorStore } from './chartCursor';
import { EmptyNote } from './Unavailable';

// uPlot draws on a canvas, so it needs real colour values rather than CSS variables. They are read
// from the tokens of the current theme when a chart is built (and it is built again when the theme
// changes); the values below are the dark theme's, used where no stylesheet is present (tests).
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

const FALLBACK = { text2: '#9aa7ac', line: '#243037', band: '#1d3144', energyFill: '#173a36', font: 'Inter, system-ui, sans-serif' } as const;
const COLOR_TOKEN: Record<ChartColor, string> = {
  link: '--link',
  linkSoft: '--link-hover',
  heart: '--heart',
  sleep: '--sleep',
  energy: '--energy',
  steps: '--steps',
  warn: '--warn',
};

/** The colours a chart draws with, from the tokens in effect on `el`. */
interface Palette {
  series: Record<ChartColor, string>;
  text: string;
  text2: string;
  bg: string;
  line: string;
  band: string;
  energyFill: string;
  font: string;
  /** A marker colour: a hex value, or `var(--token)` read from the theme. */
  resolve: (color: string) => string;
}

function readPalette(el: Element): Palette {
  const style = getComputedStyle(el);
  const token = (name: string, fallback: string) => (typeof style.getPropertyValue === 'function' ? style.getPropertyValue(name).trim() : '') || fallback;
  const series = Object.fromEntries((Object.keys(CHART_COLORS) as ChartColor[]).map((k) => [k, token(COLOR_TOKEN[k], CHART_COLORS[k])])) as Record<ChartColor, string>;
  return {
    series,
    text: token('--text', '#e8eef0'),
    text2: token('--text-2', FALLBACK.text2),
    bg: token('--bg', '#0f1417'),
    line: token('--line', FALLBACK.line),
    band: token('--band', FALLBACK.band),
    energyFill: token('--energy-fill', FALLBACK.energyFill),
    font: token('--font-body', FALLBACK.font),
    resolve: (color) => {
      const m = /^var\((--[\w-]+)\)$/.exec(color);
      return m ? token(m[1] as string, FALLBACK.text2) : color;
    },
  };
}
/** Gap between the value labels and the plot. */
const AXIS_GAP = 4;

/** A marker drawn on one point: green dot, orange square, red triangle (HRV status). */
export interface ChartMarker {
  shape: 'dot' | 'square' | 'triangle';
  /** Canvas colour: a hex value, or a token as `var(--hrv-balanced)` (read from the theme when drawn). */
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

/** A label under the plot, at an x value ("00" at 0, "12" at 12): drawn as the chart's own x axis so it lines up with the data. */
export interface XLabel {
  value: number;
  text: string;
}

/** A dashed line across the plot at `value` with a short label ("Avg: 4:36 /km"). */
export interface RefLine {
  value: number;
  label: string;
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
  /** Shown instead of the chart when the series has no reading. */
  emptyText?: string;
  /**
   * Axes: the value axis (labels on the left, faint grid lines across) is on by default, phone and
   * desktop alike. `true` adds uPlot's own x labels as well; `false` draws neither (no chart uses it).
   */
  axes?: boolean | 'y';
  /** Labels under the plot, drawn as the x axis at exactly these x values (hours of a day, dates). */
  xLabels?: XLabel[];
  /** What the y values are: decides the tick steps and how a tick is written (default whole numbers). */
  yUnit?: AxisUnit;
  /** A tiny chart in a tile: only two value labels, the lowest and highest reading. */
  sparkline?: boolean;
  /** A dashed average line with its label (activity charts). */
  refLine?: RefLine;
  /** Turn the value axis upside down: the lowest value on top (pace, where faster is lower). Area fills then reach the bottom. */
  yReverse?: boolean;
  /**
   * A pointer line shared with the other charts of the same `group` (they must have the same samples):
   * hovering or tapping one draws the line on all of them and writes the nearest sample to `store`.
   */
  cursor?: { group: string; store: CursorStore };
}

function bandExtent(band: [number, number] | BandSeries): [number, number] {
  if (Array.isArray(band)) return band;
  const vals = [...band.lo, ...band.hi].filter((v): v is number => typeof v === 'number' && Number.isFinite(v));
  return vals.length ? [Math.min(...vals), Math.max(...vals)] : [Infinity, -Infinity];
}

function drawBand(u: uPlot, band: [number, number] | BandSeries, x: number[], palette: Palette) {
  const ctx = u.ctx;
  ctx.save();
  ctx.fillStyle = palette.band;
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

function drawMarkers(u: uPlot, x: number[], series: ChartSeries[], palette: Palette) {
  const ctx = u.ctx;
  const r = 5 * uPlot.pxRatio;
  ctx.save();
  for (const s of series) {
    s.markers?.forEach((m, i) => {
      const v = s.data[i];
      if (!m || v == null) return;
      const cx = u.valToPos(x[i] as number, 'x', true);
      const cy = u.valToPos(v, 'y', true);
      ctx.fillStyle = palette.resolve(m.color);
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

/** A dashed horizontal line at `value` (an average) with its label at the right end, above the line. */
function drawRefLine(u: uPlot, ref: RefLine, palette: Palette, fs: number) {
  const ctx = u.ctx;
  const ratio = uPlot.pxRatio;
  const y = u.valToPos(ref.value, 'y', true);
  if (!Number.isFinite(y) || y < u.bbox.top || y > u.bbox.top + u.bbox.height) return;
  const right = u.bbox.left + u.bbox.width;
  ctx.save();
  ctx.strokeStyle = palette.text;
  ctx.lineWidth = Math.max(1, ratio);
  ctx.setLineDash([4 * ratio, 3 * ratio]);
  ctx.beginPath();
  ctx.moveTo(u.bbox.left, y);
  ctx.lineTo(right, y);
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.font = `${fs * ratio}px ${palette.font}`;
  ctx.textAlign = 'right';
  ctx.textBaseline = 'bottom';
  const width = ctx.measureText(ref.label).width;
  const labelY = y - 2 * ratio < u.bbox.top + fs * ratio ? y + fs * ratio + 2 * ratio : y - 2 * ratio;
  ctx.fillStyle = palette.bg;
  ctx.globalAlpha = 0.8;
  ctx.fillRect(right - width - 6 * ratio, labelY - fs * ratio - ratio, width + 6 * ratio, fs * ratio + 2 * ratio);
  ctx.globalAlpha = 1;
  ctx.fillStyle = palette.text;
  ctx.fillText(ref.label, right - 3 * ratio, labelY);
  ctx.restore();
}

function hasData(series: ChartSeries[]): boolean {
  return series.some((s) => s.data.some((v) => typeof v === 'number' && Number.isFinite(v)));
}

/** Line chart on uPlot with an accessible name and a text summary. */
export function SeriesChart({ label, summary, x, series, height = 90, time = false, band, yRange, xRange, summaryVisible = false, emptyText, axes = 'y', xLabels, yUnit = 'int', sparkline = false, refLine, yReverse = false, cursor }: SeriesChartProps) {
  const host = useRef<HTMLDivElement>(null);
  const theme = useTheme();
  const available = hasData(series) && x.length > 0;
  const cursorGroup = cursor?.group;
  const cursorStore = cursor?.store;
  const key = useMemo(
    () => JSON.stringify([x, series, height, time, band, yRange, xRange, axes, xLabels, yUnit, sparkline, refLine, yReverse, theme, cursorGroup]),
    [x, series, height, time, band, yRange, xRange, axes, xLabels, yUnit, sparkline, refLine, yReverse, theme, cursorGroup],
  );
  useEffect(() => {
    const el = host.current;
    if (!el || !available) return;
    const width = Math.max(120, el.clientWidth || 320);

    // Canvas text ignores the page text size, so take it from the element (styled with --fs-xs): at
    // 130% text zoom the labels grow with the rest of the page. The gutter is sized from the labels.
    // Whole pixels: uPlot scales the size in the font string with a pattern that only reads integers.
    const palette = readPalette(el);
    const fs = Math.max(10, Math.round(parseFloat(getComputedStyle(el).fontSize) || 12));
    const font = `${fs}px ${palette.font}`;
    const showY = axes !== false;
    const showX = axes === true || (axes !== false && !!xLabels?.length);
    // The x labels sit half a line lower, clear of the lowest value label that is centred on the baseline.
    const xGap = Math.ceil(fs / 2) + 2;
    const xSize = showX ? Math.ceil(fs * 1.2) + xGap + 2 : 0;
    // Half a label of room above the top tick and, with no x labels, below the bottom one.
    const padTop = Math.ceil(fs / 2) + 2;
    const padBottom = showX ? 2 : Math.ceil(fs / 2) + 2;
    const plotHeight = sparkline ? Math.max(height, Math.ceil(fs * 1.6) + padTop + padBottom) : height;
    const total = plotHeight + xSize;
    const ticksWanted = tickCountFor(plotHeight - padTop - padBottom, fs);
    el.style.minHeight = `${total}px`;

    // uPlot remembers the font it last set, so put the context's font back after measuring.
    const measure = (u: uPlot, text: string) => {
      const ctx = u.ctx;
      const before = ctx.font;
      ctx.font = font;
      const width = ctx.measureText(text).width;
      ctx.font = before;
      return width;
    };
    const labelText = new Map((xLabels ?? []).map((l) => [l.value, l.text]));
    const lastLabel = xLabels?.reduce<XLabel | undefined>((a, l) => (!a || l.value > a.value ? l : a), undefined);

    const opts: uPlot.Options = {
      width,
      height: total,
      // The right side keeps room for half of the last x label, which is centred on the plot's edge.
      padding: [padTop, (u: uPlot) => (lastLabel ? Math.max(6, Math.ceil(measure(u, lastLabel.text) / 2) + 2) : 6), padBottom, 0],
      legend: { show: false },
      // Off unless the chart shares a pointer line. Then: a vertical line and a dot per series, no zoom by dragging.
      cursor: cursorGroup
        ? {
            x: true,
            y: false,
            drag: { x: false, y: false, setScale: false },
            points: { size: 8, width: 2, stroke: (u: uPlot, i: number) => (u.series[i]?.stroke as string | undefined) ?? palette.text, fill: palette.bg },
            sync: { key: cursorGroup, scales: ['x', null] },
          }
        : { show: false },
      select: { show: false, left: 0, top: 0, width: 0, height: 0 },
      scales: {
        x: xRange ? { time, range: () => xRange } : { time },
        y: {
          ...(yReverse ? { dir: -1 as const } : {}),
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
        {
          show: showX,
          stroke: palette.text2,
          font,
          gap: xGap,
          grid: { show: false },
          ticks: { show: false },
          size: xSize,
          ...(xLabels?.length
            ? {
                splits: (_u: uPlot, _i: number, min: number, max: number) => xLabels.map((l) => l.value).filter((v) => v >= min - 1e-9 && v <= max + 1e-9),
                values: (_u: uPlot, splits: number[]) => splits.map((v) => labelText.get(v) ?? ''),
              }
            : {}),
        },
        {
          show: showY,
          stroke: palette.text2,
          font,
          gap: AXIS_GAP,
          grid: { stroke: palette.line, width: 1 },
          ticks: { show: false },
          // As wide as the widest label, so numbers never clip (and a 130% font widens it).
          size: (u: uPlot, values: string[] | null) => (values && values.length ? Math.ceil(Math.max(...values.map((v) => measure(u, v)))) + AXIS_GAP + 2 : 24),
          splits: (_u: uPlot, _i: number, min: number, max: number) =>
            (sparkline ? [...new Set(extremeTicks(series.flatMap((s) => s.data)).map((v) => roundTick(v, yUnit)))] : unitTicks(min, max, ticksWanted, yUnit)).filter(
              (v) => v >= min && v <= max,
            ),
          values: (_u: uPlot, splits: number[]) => formatTicks(splits, yUnit),
        },
      ],
      series: [
        {},
        ...series.map((s): uPlot.Series => {
          const stroke = palette.series[s.color];
          return {
            label: s.label,
            stroke,
            width: s.width ?? 2.5,
            points: s.points ? { show: true, size: 7, width: 0, fill: stroke } : { show: false },
            spanGaps: false,
            ...(s.noLine ? { paths: () => null } : {}),
            ...(s.area ? { fill: s.color === 'energy' ? palette.energyFill : `${stroke}33`, ...(yReverse ? { fillTo: (u: uPlot) => u.scales['y']?.max ?? 0 } : {}) } : {}),
          };
        }),
      ],
      hooks: {
        ...(cursorGroup && cursorStore
          ? {
              setCursor: [
                (u: uPlot) => {
                  const on = typeof u.cursor.left === 'number' && u.cursor.left >= 0;
                  cursorStore.set(on ? (u.cursor.idx ?? null) : null);
                },
              ],
            }
          : {}),
        ...(band ? { drawClear: [(u: uPlot) => drawBand(u, band, x, palette)] } : {}),
        ...(series.some((s) => s.markers) || refLine
          ? {
              draw: [
                (u: uPlot) => {
                  if (series.some((s) => s.markers)) drawMarkers(u, x, series, palette);
                  if (refLine) drawRefLine(u, refLine, palette, fs);
                },
              ],
            }
          : {}),
      },
    };

    const plot = new uPlot(opts, [x, ...series.map((s) => s.data)] as uPlot.AlignedData, el);
    const observer = new ResizeObserver(() => {
      const w = Math.max(120, el.clientWidth);
      if (w !== plot.width) plot.setSize({ width: w, height: total });
    });
    observer.observe(el);
    // The labels are measured with the web font; when it arrives late, measure again.
    let disposed = false;
    void document.fonts?.ready.then(() => {
      if (!disposed) plot.redraw?.(false, true);
    });
    return () => {
      disposed = true;
      observer.disconnect();
      plot.destroy();
    };
    // `key` captures every input that changes the plot.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, available]);

  if (!available) {
    return (
      <>
        {/* A visible summary already says what is missing; otherwise one short sentence does. */}
        {summaryVisible ? <p className="chart-summary">{summary}</p> : <EmptyNote>{emptyText ?? 'Nothing recorded for this period.'}</EmptyNote>}
      </>
    );
  }

  return (
    <div className="series-chart">
      <div ref={host} className="series-chart__plot" role="img" aria-label={label} style={{ minHeight: axes === true || xLabels?.length ? height + 28 : height }} />
      <p className={summaryVisible ? 'chart-summary' : 'sr-only'}>{summary}</p>
    </div>
  );
}
