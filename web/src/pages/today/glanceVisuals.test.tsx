import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HomeCard, Today } from '../../api/generated';
import { StackedBars } from '../../components';
import { GlanceCard } from './cards';
import { makeToday } from './testData';

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
const D = (date: string, value: number | null) => ({ date, value });

function stubApi() {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      if (url.startsWith('/api/metrics/steps')) {
        return json({ sample: true, metric: 'steps', range: '7d', from: '2026-11-13', to: '2026-11-19', bucket: 'day', series: [{ key: 'steps', label: 'Steps', unit: 'steps', points: [9000, 4000, null, 8000, 8100, 100, 6240].map((v, i) => D(`2026-11-${13 + i}`, v)) }], stats: [], goal: 8000, target_low: null, target_high: null, has_prev: true, has_next: false });
      }
      if (url.startsWith('/api/training/status')) {
        const days = Array.from({ length: 28 }, (_, i) => ({ date: `2026-10-${String(23 + i).padStart(2, '0')}`, status: i < 10 ? null : { code: 4, label: 'Productive', kind: 'productive' } }));
        return json({ sample: true, from: '2026-10-23', to: '2026-11-19', today: '2026-11-19', days, runs: [], legend: [], current: {}, load: {} });
      }
      return json({ error: 'no' }, 404);
    }),
  );
}

function renderCard(id: HomeCard, today: Today) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <GlanceCard id={id} today={today} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const visual = (kind: string) => document.querySelector(`[data-visual="${kind}"]`);
const base = makeToday();
const series = (n: number) => Array.from({ length: n }, (_, i) => ({ time: `2026-11-19T0${i % 10}:00:00+00:00`, value: 60 + i }));

afterEach(() => vi.unstubAllGlobals());

describe('glance card pictures', () => {
  it('draws today\'s heart rate when there are samples, and nothing without', () => {
    const { unmount } = renderCard('heart_rate', { ...base, heart_rate: { ...base.heart_rate, day: { ...base.heart_rate.day, series: series(5) } } });
    const box = visual('heart_rate');
    expect(box).not.toBeNull();
    expect(box?.getAttribute('aria-hidden')).toBe('true');
    expect(box?.querySelector('line[stroke-dasharray]')).not.toBeNull();
    unmount();
    renderCard('heart_rate', base);
    expect(visual('heart_rate')).toBeNull();
    expect(screen.getByText('51 bpm')).toBeInTheDocument();
  });

  it('draws a stress ring for the average, and none without one', () => {
    const { unmount } = renderCard('stress', base);
    expect(visual('stress')?.querySelector('circle:nth-of-type(2)')?.getAttribute('style')).toContain('--stress-rest');
    unmount();
    renderCard('stress', { ...base, stress: { ...base.stress, average: 80 } });
    expect(visual('stress')?.querySelector('circle:nth-of-type(2)')?.getAttribute('style')).toContain('--stress-high');
    document.body.innerHTML = '';
    renderCard('stress', { ...base, stress: { ...base.stress, average: null } });
    expect(visual('stress')).toBeNull();
  });

  it('draws the intensity ring, the load range bar and the recovery ring', () => {
    renderCard('intensity_minutes', base);
    expect(visual('intensity_minutes')).not.toBeNull();
    renderCard('training_load', base);
    expect(visual('training_load')?.querySelector('.progress__marker')).not.toBeNull();
    renderCard('recovery_time', base);
    expect(visual('recovery_time')).not.toBeNull();
  });

  it('leaves out the load bar without a target range', () => {
    renderCard('training_load', { ...base, training_load: { ...base.training_load, target_min: null, target_max: null } });
    expect(visual('training_load')).toBeNull();
  });

  it('draws VO2 max over three months only with readings', () => {
    const { unmount } = renderCard('vo2_max', { ...base, vo2max: { ...base.vo2max, series_3m: [D('2026-10-01', 45.7), D('2026-11-18', 46.2)] } });
    expect(visual('vo2_max')).not.toBeNull();
    unmount();
    renderCard('vo2_max', base);
    expect(visual('vo2_max')).toBeNull();
  });

  it('draws seven step bars, full colour for the days that reached the goal', async () => {
    stubApi();
    renderCard('steps', base);
    await waitFor(() => expect(visual('steps_week')?.querySelectorAll('.gv__bar')).toHaveLength(7));
    const bars = Array.from(visual('steps_week')!.querySelectorAll<HTMLElement>('.gv__bar'));
    expect(bars.map((b) => b.style.background)).toEqual(['var(--steps)', 'var(--steps-dim)', '', 'var(--steps)', 'var(--steps)', 'var(--steps-dim)', 'var(--steps-dim)']);
    expect(screen.getByText('6,240')).toBeInTheDocument();
  });

  it('draws 28 blocks of training status, coloured by status', async () => {
    stubApi();
    renderCard('training_status', { ...base, training_status: { ...base.training_status, label: 'Productive', kind: 'productive' } });
    await waitFor(() => expect(visual('training_status')?.querySelectorAll('.gv__block')).toHaveLength(28));
    const kinds = Array.from(visual('training_status')!.querySelectorAll('.gv__block')).map((b) => b.getAttribute('data-kind'));
    expect(kinds.slice(0, 10)).toEqual(Array(10).fill('none'));
    expect(kinds.slice(10)).toEqual(Array(18).fill('productive'));
  });

  it('shows no step bars when the week cannot be loaded', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => json({ error: 'x' }, 500)));
    renderCard('steps', base);
    await waitFor(() => expect(visual('steps_week')).toBeNull());
  });
});

describe('StackedBars', () => {
  it('stacks the parts: the tallest bar reaches the top and each part has its colour', () => {
    const { container } = render(
      <StackedBars
        parts={[
          { key: 'a', color: 'red', values: [60, 120, null] },
          { key: 'b', color: 'blue', values: [30, 0, null] },
        ]}
        labels={['M', 'T', 'W']}
        label="Test"
      />,
    );
    const rects = Array.from(container.querySelectorAll('svg rect'));
    // Bar 1: two parts, bar 2: one part, bar 3: an empty stub.
    expect(rects).toHaveLength(4);
    const fills = rects.map((r) => (r as SVGElement).style.fill);
    expect(fills).toEqual(['red', 'blue', 'red', 'var(--track)']);
  });
});
