import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// uPlot draws on a canvas, which jsdom does not have: replace it with a recorder of its options.
interface AxisOpts {
  show: boolean;
  font: string;
  grid: { show?: boolean; stroke?: string };
  size: number | ((u: unknown, values: string[] | null) => number);
  splits?: (u: unknown, i: number, min: number, max: number) => number[];
  values?: (u: unknown, splits: number[]) => string[];
}
interface Opts {
  height: number;
  scales: { x: { range?: unknown } };
  axes: [AxisOpts, AxisOpts];
}
const created: Opts[] = [];
vi.mock('uplot', () => ({
  default: class {
    width = 100;
    constructor(opts: Opts) {
      created.push(opts);
    }
    setSize() {}
    destroy() {}
  },
}));
vi.mock('uplot/dist/uPlot.min.css', () => ({}));

import { SeriesChart } from './SeriesChart';

/** A canvas context that measures every character as 7 px wide. */
const fakeUplot = { ctx: { font: '', measureText: (t: string) => ({ width: t.length * 7 }) } };

const series = [{ label: 'HR', color: 'heart' as const, data: [60, null, 80] }];

describe('SeriesChart', () => {
  beforeEach(() => {
    created.length = 0;
  });

  it('uses a fixed x range when given one (a day chart spans 0 to 24 hours)', () => {
    render(<SeriesChart label="Heart rate" summary="s" x={[6, 8, 12]} xRange={[0, 24]} series={series} />);
    const range = created[0]?.scales.x.range as (() => [number, number]) | undefined;
    expect(range?.()).toEqual([0, 24]);
  });

  it('fits the x range to the data by default', () => {
    render(<SeriesChart label="Heart rate" summary="s" x={[6, 8, 12]} series={series} />);
    expect(created[0]?.scales.x.range).toBeUndefined();
  });

  it('summary is for screen readers unless asked to be visible', () => {
    const { rerender } = render(<SeriesChart label="Heart rate" summary="Peak 148 bpm." x={[1, 2, 3]} series={series} />);
    expect(screen.getByText('Peak 148 bpm.')).toHaveClass('sr-only');
    rerender(<SeriesChart label="Heart rate" summary="Peak 148 bpm." summaryVisible x={[1, 2, 3]} series={series} />);
    expect(screen.getByText('Peak 148 bpm.')).toHaveClass('chart-summary');
  });

  it('without data it shows the visible summary, or else one short sentence, never "unavailable"', () => {
    const empty = [{ label: 'HR', color: 'heart' as const, data: [] }];
    const { rerender } = render(<SeriesChart label="Heart rate" summary="No heart rate today." summaryVisible x={[]} series={empty} />);
    expect(screen.getByText('No heart rate today.')).toBeInTheDocument();
    expect(screen.queryByText('unavailable')).not.toBeInTheDocument();
    rerender(<SeriesChart label="Heart rate" summary="No heart rate today." x={[]} series={empty} />);
    expect(screen.getByText('Nothing recorded for this period.')).toBeInTheDocument();
    rerender(<SeriesChart label="Heart rate" summary="No heart rate today." emptyText="No heart rate recorded." x={[]} series={empty} />);
    expect(screen.getByText('No heart rate recorded.')).toBeInTheDocument();
    expect(created).toHaveLength(0);
  });

  describe('value axis', () => {
    const hr = [{ label: 'HR', color: 'heart' as const, data: [60, 90, 150] }];

    it('is on by default with grid lines, 3 to 5 round labels, and a gutter as wide as the widest one', () => {
      render(<SeriesChart label="Heart rate" summary="s" x={[0, 1, 2]} series={hr} height={110} />);
      const y = created[0]!.axes[1];
      expect(y.show).toBe(true);
      expect(y.grid.stroke).toBe('#243037');
      const splits = y.splits!(fakeUplot, 1, 48, 162);
      expect(splits.length).toBeGreaterThanOrEqual(3);
      expect(splits.length).toBeLessThanOrEqual(5);
      expect(splits.every((v) => v >= 48 && v <= 162 && Number.isInteger(v))).toBe(true);
      const labels = y.values!(fakeUplot, [50, 100, 150]);
      expect(labels).toEqual(['50', '100', '150']);
      // 3 characters at 7 px, plus the 4 px gap and 2 px of air.
      expect((y.size as (u: unknown, v: string[] | null) => number)(fakeUplot, labels)).toBe(21 + 4 + 2);
      expect((y.size as (u: unknown, v: string[] | null) => number)(fakeUplot, ['5', '10'])).toBe(14 + 4 + 2);
    });

    it('takes the label size from the element font size, so text zoom grows the labels', () => {
      const spy = vi.spyOn(window, 'getComputedStyle').mockReturnValue({ fontSize: '15.6px' } as CSSStyleDeclaration);
      render(<SeriesChart label="Heart rate" summary="s" x={[0, 1, 2]} series={hr} />);
      spy.mockRestore();
      expect(created[0]!.axes[1].font).toMatch(/^16px /);
      expect(created[0]!.axes[0].font).toMatch(/^16px /);
    });

    it('measures labels without leaving its font on the canvas (uPlot caches the font it set)', () => {
      render(<SeriesChart label="Heart rate" summary="s" x={[0, 1, 2]} series={hr} />);
      const ctx = { font: '24px Inter', measureText: (t: string) => ({ width: t.length * 7 }) };
      (created[0]!.axes[1].size as (u: unknown, v: string[]) => number)({ ctx }, ['50', '100']);
      expect(ctx.font).toBe('24px Inter');
    });

    it('writes pace as m:ss and durations as hours', () => {
      render(<SeriesChart label="Pace" summary="s" x={[0, 1]} series={[{ label: 'Pace', color: 'link', data: [5.4, 6.1] }]} yUnit="pace" />);
      expect(created[0]!.axes[1].values!(fakeUplot, [5.5, 6])).toEqual(['5:30', '6:00']);
      render(<SeriesChart label="Sleep" summary="s" x={[0, 1]} series={[{ label: 'Sleep', color: 'sleep', data: [6, 7.5] }]} yUnit="hours" />);
      expect(created[1]!.axes[1].values!(fakeUplot, [6, 7])).toEqual(['6h', '7h']);
    });

    it('labels only the lowest and highest reading of a sparkline', () => {
      render(<SeriesChart label="Breathing" summary="s" x={[0, 1, 2, 3]} series={[{ label: 'B', color: 'link', data: [14.2, null, 15.8, 15] }]} height={36} yUnit="decimal" sparkline />);
      const y = created[0]!.axes[1];
      expect(y.splits!(fakeUplot, 1, 13.5, 16.5)).toEqual([14.2, 15.8]);
      expect(y.values!(fakeUplot, [14.2, 15.8])).toEqual(['14.2', '15.8']);
    });

    it('can be switched off', () => {
      render(<SeriesChart label="Heart rate" summary="s" x={[0, 1, 2]} series={hr} axes={false} />);
      expect(created[0]!.axes[1].show).toBe(false);
    });
  });

  describe('x labels', () => {
    const energy = [{ label: 'Energy', color: 'energy' as const, data: [20, 80, 40] }];
    const hours = [0, 6, 12, 18, 24].map((value) => ({ value, text: String(value).padStart(2, '0') }));

    it('draws the given labels as the x axis, at exactly their x values', () => {
      render(<SeriesChart label="Energy" summary="s" x={[1, 5, 9]} xRange={[0, 24]} xLabels={hours} series={energy} />);
      const x = created[0]!.axes[0];
      expect(x.show).toBe(true);
      expect(x.grid.show).toBe(false);
      expect(x.splits!(fakeUplot, 0, 0, 24)).toEqual([0, 6, 12, 18, 24]);
      expect(x.values!(fakeUplot, [0, 12, 24])).toEqual(['00', '12', '24']);
      // Labels outside the visible range are dropped rather than drawn off the plot.
      expect(x.splits!(fakeUplot, 0, 0, 12)).toEqual([0, 6, 12]);
    });

    it('has no x axis without labels, and leaves room under the plot for the lowest value label', () => {
      render(<SeriesChart label="Energy" summary="s" x={[1, 5, 9]} series={energy} height={100} />);
      expect(created[0]!.axes[0].show).toBe(false);
      expect(created[0]!.height).toBe(100);
    });

    it('adds the x axis height to the chart, so the plot keeps the height asked for', () => {
      render(<SeriesChart label="Energy" summary="s" x={[1, 5, 9]} xLabels={hours} series={energy} height={100} />);
      expect(created[0]!.height).toBeGreaterThan(100);
    });
  });
});
