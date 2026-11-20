import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { BarSeries } from './BarSeries';
import { evenLabels, PlotFrame, PlotXLabels } from './PlotFrame';
import { StageTimeline, StressBars } from './StageBar';
import { WeightChart } from '../pages/body/WeightChart';

/** The value labels in a chart's left gutter (the invisible width-sizer excluded). */
const yLabels = (root: HTMLElement = document.body) => Array.from(root.querySelectorAll('.plot__y-tick')).map((e) => e.textContent);
const gridLines = (root: HTMLElement = document.body) => Array.from(root.querySelectorAll('.plot__area svg line'));

describe('PlotFrame', () => {
  it('puts each label and its grid line at the same place, with the x labels in the plot column', () => {
    render(
      <PlotFrame
        ticks={[
          { label: '0', at: 1 },
          { label: '50', at: 0.5 },
          { label: '100', at: 0 },
        ]}
        viewW={320}
        viewH={80}
        label="Test chart"
        xAxis={<PlotXLabels labels={evenLabels(['a', 'b', 'c'])} />}
      >
        <rect />
      </PlotFrame>,
    );
    const chart = screen.getByRole('img', { name: 'Test chart' });
    expect(yLabels(chart)).toEqual(['0', '50', '100']);
    const ticks = Array.from(chart.querySelectorAll<HTMLElement>('.plot__y-tick'));
    expect(ticks.map((t) => t.style.top)).toEqual(['100%', '50%', '0%']);
    const lines = gridLines(chart);
    expect(lines.map((l) => l.getAttribute('y1'))).toEqual(['80', '40', '0']);
    // Every line spans the whole plot and keeps a hairline width when the SVG stretches.
    for (const l of lines) {
      expect(l.getAttribute('x2')).toBe('320');
      expect(l.getAttribute('vector-effect')).toBe('non-scaling-stroke');
    }
    // The widest label sizes the gutter, so a longer number never clips.
    expect(chart.querySelector('.plot__y-size')?.textContent).toBe('100');
    // x labels share the plot's grid column.
    expect(chart.querySelector('.plot__x .time-axis')).not.toBeNull();
    const xs = Array.from(chart.querySelectorAll<HTMLElement>('.time-axis span'));
    expect(xs.map((s) => [s.textContent, s.style.left])).toEqual([
      ['a', '0%'],
      ['b', '50%'],
      ['c', '100%'],
    ]);
  });

  it('keeps room under the plot when there are no x labels', () => {
    render(<PlotFrame ticks={[{ label: '0', at: 1 }]} viewW={10} viewH={10} label="Bare">{null}</PlotFrame>);
    expect(screen.getByRole('img', { name: 'Bare' })).toHaveClass('plot--bare');
  });
});

describe('BarSeries value axis', () => {
  const labels = ['Mon', 'Tue', 'Wed', 'Thu'];

  it('labels the scale with round whole numbers and draws a grid line at each', () => {
    render(<BarSeries values={[9120, 7480, 8360, 6240]} labels={labels} label="Steps" height={100} />);
    const shown = yLabels();
    expect(shown.length).toBeGreaterThanOrEqual(3);
    expect(shown.length).toBeLessThanOrEqual(5);
    expect(shown[shown.length - 1]).toMatch(/^[0-9,]+$/);
    expect(gridLines()).toHaveLength(shown.length);
    // The scale starts at zero and reaches the tallest bar.
    expect(shown).toContain('0');
    expect(Number(shown[shown.length - 1]!.replace(',', ''))).toBeGreaterThanOrEqual(9120);
  });

  it('writes sleep minutes as hours', () => {
    render(<BarSeries values={[385, 412, 365, 450]} labels={labels} label="Sleep" yUnit="minutes" height={100} />);
    const shown = yLabels();
    expect(shown.every((t) => t!.endsWith('h'))).toBe(true);
    expect(shown[0]).toBe('0h');
  });

  it('puts the goal line and the tick on the same scale, and the labels in the bars column', () => {
    render(<BarSeries values={[9120, 7480]} labels={['Mon', 'Tue']} label="Steps" goal={8000} max={10000} height={100} />);
    const goal = document.querySelector('line[stroke-dasharray]')!;
    expect(Number(goal.getAttribute('y1'))).toBeCloseTo(100 - (8000 / 10000) * 96, 5);
    expect(document.querySelector('.plot__x .chart__axis')).not.toBeNull();
    // The last tick is at the top of the 10,000 scale.
    expect(yLabels()).toContain('10,000');
  });
});

describe('StressBars value axis', () => {
  it('has a 0 to 100 scale and the hour labels under the bars', () => {
    render(<StressBars buckets={[{ level: 'low', value: 20 }, { level: 'high', value: 80 }]} label="Stress" xLabels={['00', '12', '24']} />);
    const shown = yLabels();
    expect(shown[0]).toBe('0');
    expect(shown[shown.length - 1]).toBe('100');
    expect(Array.from(document.querySelectorAll('.time-axis span')).map((s) => s.textContent)).toEqual(['00', '12', '24']);
  });
});

describe('StageTimeline rows', () => {
  it('labels the four stage rows on the left', () => {
    render(<StageTimeline spans={[{ stage: 'deep', from: 0, to: 60 }, { stage: 'rem', from: 60, to: 120 }]} totalMinutes={120} label="Stages" ticks={['23:00', '01:00']} />);
    expect(yLabels()).toEqual(['Awake', 'REM', 'Light', 'Deep']);
    expect(gridLines()).toHaveLength(4);
    expect(Array.from(document.querySelectorAll('.time-axis span')).map((s) => s.textContent)).toEqual(['23:00', '01:00']);
  });

  it('keeps at most three time labels so they never touch', () => {
    render(<StageTimeline spans={[{ stage: 'deep', from: 0, to: 60 }]} totalMinutes={120} label="Stages" ticks={['23:29', '01:22', '03:16', '05:09', '07:03']} />);
    expect(Array.from(document.querySelectorAll('.time-axis span')).map((s) => s.textContent)).toEqual(['23:29', '03:16', '07:03']);
  });
});

describe('WeightChart value axis', () => {
  const t = (day: number, v: number) => ({ time: `2026-11-${String(day).padStart(2, '0')}T07:00:00Z`, value: v });

  it('shows kilograms on the left with grid lines, and the dates in the plot column', () => {
    render(<WeightChart weights={[t(1, 82.4), t(8, 81.6), t(15, 80.9)]} trend={[t(1, 82.2), t(8, 81.7), t(15, 81)]} />);
    const chart = screen.getByRole('img', { name: /Weight trend over 90 days/ });
    const shown = yLabels(chart);
    expect(shown.length).toBeGreaterThanOrEqual(3);
    for (const label of shown) {
      const kg = Number(label);
      expect(kg).toBeGreaterThan(79);
      expect(kg).toBeLessThan(84);
    }
    expect(gridLines(chart)).toHaveLength(shown.length);
    expect(within(chart).getAllByText(/\d+ Nov/).length).toBeGreaterThanOrEqual(2);
    // One round dot per weigh-in, in the plot (not stretched with the SVG).
    expect(chart.querySelectorAll('.plot__area > span')).toHaveLength(3);
  });
});
