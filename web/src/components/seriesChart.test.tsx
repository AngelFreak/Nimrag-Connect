import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// uPlot draws on a canvas, which jsdom does not have: replace it with a recorder of its options.
const created: { scales: { x: { range?: unknown } } }[] = [];
vi.mock('uplot', () => ({
  default: class {
    width = 100;
    constructor(opts: { scales: { x: { range?: unknown } } }) {
      created.push(opts);
    }
    setSize() {}
    destroy() {}
  },
}));
vi.mock('uplot/dist/uPlot.min.css', () => ({}));

import { SeriesChart } from './SeriesChart';

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

  it('without data it says "unavailable" and still shows a visible summary', () => {
    render(<SeriesChart label="Heart rate" summary="No heart rate today." summaryVisible x={[]} series={[{ label: 'HR', color: 'heart', data: [] }]} />);
    expect(screen.getByText('unavailable')).toBeInTheDocument();
    expect(screen.getByText('No heart rate today.')).toBeInTheDocument();
    expect(created).toHaveLength(0);
  });
});
