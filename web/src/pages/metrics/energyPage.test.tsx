import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, within } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DayResponse } from '../../api/generated/DayResponse';
import type { MetricRange } from '../../api/generated/MetricRange';
import { addDays, browserToday } from '../../lib/dates';
import { MetricPage } from '../MetricPage';

// uPlot needs a real canvas, so the range chart is replaced by its name and text alternative.
vi.mock('../../components/SeriesChart', () => ({
  SeriesChart: ({ label, summary }: { label: string; summary: string }) => (
    <div role="img" aria-label={label}>
      <p className="sr-only">{summary}</p>
    </div>
  ),
}));

// The Energy page: the day as one timeline (energy line, stress band, sleep block, activity blocks)
// with its numbers and one sentence, and charged and drained per day on the longer tabs.

const TODAY = browserToday();
const at = (h: number, m = 0) => `${TODAY}T${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:00+00:00`;

/** Hourly energy: up while asleep (00 to 06), then a run drains it, then a quiet decline. */
const LINE = [30, 45, 60, 75, 85, 92, 95, 90, 60, 30, 28, 26, 25, 24];

function day(over: Partial<DayResponse> = {}): DayResponse {
  return {
    sample: false,
    date: TODAY,
    previous_date: addDays(TODAY, -1),
    next_date: null,
    heart_rate: { resting_hr_bpm: 51, avg_7d_resting_hr_bpm: 52, day: { low_bpm: 45, high_bpm: 148, series: [] } },
    stress: {
      average: 28,
      main_level: 'low',
      rest_s: 3600,
      low_s: 7200,
      medium_s: 1800,
      high_s: 0,
      activity_s: 600,
      series: LINE.map((_, i) => ({ time: at(i), stress: 20 + i, level: 'low' as const })),
    },
    energy: {
      now: 24,
      now_at: at(13),
      charged: 65,
      drained: 71,
      high: 95,
      low: 24,
      wake_time: at(6),
      series: LINE.map((value, i) => ({ time: at(i), value })),
    },
    steps: 5609,
    step_goal: 8000,
    intensity_minutes: 19,
    breathing: { lowest: null, highest: null, avg_awake: null, avg_asleep: null, hourly: [] },
    spo2: { average: null, lowest: null, latest: null, latest_at: null, avg_asleep: null, hourly: [] },
    sleep: { date: TODAY, start: addDays(TODAY, -1) + 'T22:30:00+00:00', end: at(6), duration_s: 27000, score: 82, deep_s: null, light_s: null, rem_s: null, awake_s: null },
    activities: [
      { id: 'run1', kind: 'run', sport_name: 'running', sub_sport_name: null, start: at(7, 30), date: TODAY, duration_s: 5400, distance_m: 8000, avg_pace_s_per_km: 330, avg_hr_bpm: 150, max_hr_bpm: 170, calories_kcal: 500, ascent_m: 30, sets: null, volume_kg: null, exercises: null },
    ],
    ...over,
  };
}

function mockApi(routes: Record<string, unknown>) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      const hit = Object.entries(routes).find(([prefix]) => url.startsWith(prefix));
      if (!hit) return new Response(JSON.stringify({ error: `no mock for ${url}` }), { status: 404, headers: { 'content-type': 'application/json' } });
      return new Response(JSON.stringify(hit[1]), { status: 200, headers: { 'content-type': 'application/json' } });
    }),
  );
}

function renderAt(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/metrics/:metric" element={<MetricPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.unstubAllGlobals());

describe('Energy, one day', () => {
  it('shows the numbers of today in four tiles: now, charged, drained and lowest (the header has the highest)', async () => {
    mockApi({ [`/api/days/${TODAY}`]: day() });
    renderAt('/metrics/energy?range=1d');
    await screen.findByRole('heading', { level: 1, name: 'Energy' });
    const tiles = await screen.findAllByText(/^(Now|Charged today|Drained today|Highest|Lowest)$/);
    expect(tiles.map((t) => t.textContent)).toEqual(['Now', 'Charged today', 'Drained today', 'Lowest']);
    const body = document.body.textContent ?? '';
    expect(body).toContain('+65');
    expect(body).toContain('−71');
  });

  it('draws one combined timeline with the night shaded and the run as a link to its activity', async () => {
    mockApi({ [`/api/days/${TODAY}`]: day() });
    renderAt('/metrics/energy?range=1d');
    const chart = await screen.findByRole('img', { name: /Energy through the day from 0 to 100.*time asleep shaded/ });
    expect(chart).toBeInTheDocument();
    expect(document.querySelector('.day-chart rect[style*="--sleep"]')).not.toBeNull();
    const lane = screen.getByRole('list', { name: 'Activities of the day' });
    const link = within(lane).getByRole('link', { name: /^Run, 07:30, 90 min$/ });
    expect(link).toHaveAttribute('href', '/activities/run1');
    expect(screen.getByText('Asleep')).toBeInTheDocument();
  });

  it('says in one sentence how the day went, from the numbers', async () => {
    mockApi({ [`/api/days/${TODAY}`]: day() });
    renderAt('/metrics/energy?range=1d');
    const sentence = await screen.findByText(/^Sleep recharged you by/);
    // Asleep until 06:00: rises of 15, 15, 15, 10, 7 and 3 hourly steps inside the night.
    expect(sentence).toHaveTextContent('Sleep recharged you by 65.');
    expect(sentence).toHaveTextContent('Your run drained 60 of the 71 used.');
    expect(sentence).toHaveTextContent('It is at 24 now.');
  });

  it('leaves out the lane and the sleep block on a day without a night or activities', async () => {
    mockApi({ [`/api/days/${TODAY}`]: day({ sleep: null, activities: [] }) });
    renderAt('/metrics/energy?range=1d');
    await screen.findByRole('img', { name: /Energy through the day/ });
    expect(screen.queryByRole('list', { name: 'Activities of the day' })).not.toBeInTheDocument();
    expect(document.querySelector('.day-chart rect[style*="--sleep"]')).toBeNull();
    expect(screen.queryByText(/^Sleep recharged/)).not.toBeInTheDocument();
  });
});

describe('Energy, 7 days', () => {
  const week = (from: string): MetricRange => {
    const dates = Array.from({ length: 7 }, (_, i) => addDays(from, i));
    const s = (key: string, label: string, f: (i: number) => number | null) => ({ key, label, unit: '', points: dates.map((date, i) => ({ date, value: f(i) })) });
    return {
      sample: false,
      metric: 'energy',
      range: '7d',
      from,
      to: TODAY,
      bucket: 'day',
      series: [s('high', 'High', () => 90), s('low', 'Low', () => 15), s('charged', 'Charged', (i) => (i === 2 ? null : 70 + i)), s('drained', 'Drained', (i) => (i === 2 ? null : 60 + i))],
      stats: [],
      goal: null,
      target_low: null,
      target_high: null,
      has_prev: true,
      has_next: false,
    };
  };

  it('adds charged and drained per day under the range of highs and lows, with a text alternative', async () => {
    mockApi({ '/api/metrics/energy': week(addDays(TODAY, -6)) });
    renderAt('/metrics/energy?range=7d');
    expect(await screen.findByText('Charged and drained per day')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Energy charged and drained per day' })).toBeInTheDocument();
    expect(screen.getByText(/^Energy charged and drained per day: /)).toHaveTextContent('+70 charged, 60 drained');
    expect(screen.getByText('Charged')).toBeInTheDocument();
    expect(screen.getByText('Drained')).toBeInTheDocument();
  });
});
