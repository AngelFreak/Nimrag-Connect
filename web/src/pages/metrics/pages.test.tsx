import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DayResponse } from '../../api/generated/DayResponse';
import type { MetricRange } from '../../api/generated/MetricRange';
import type { MovementResponse } from '../../api/generated/MovementResponse';
import { addDays, browserToday } from '../../lib/dates';
import { MetricPage } from '../MetricPage';
import { METRICS, weekStart, type MetricKey } from './period';

// Page tests for the metric detail pages: every metric opens on a tab, the period control and tabs
// write to the address, a row drills down, and the empty and error states say so. uPlot needs a real
// canvas, so the chart is replaced by its name and text alternative.
vi.mock('../../components/SeriesChart', () => ({
  SeriesChart: ({ label, summary, series }: { label: string; summary: string; series: { data: (number | null)[] }[] }) =>
    series.some((s) => s.data.some((v) => v !== null)) ? (
      <div role="img" aria-label={label}>
        <p className="sr-only">{summary}</p>
      </div>
    ) : (
      <p>unavailable</p>
    ),
}));

const TODAY = browserToday();
const day = (n: number) => addDays(TODAY, -n);

type Vals = (number | null)[];
const series = (key: string, from: string, vals: Vals, step = 1) => ({
  key,
  label: key,
  unit: '',
  points: vals.map((value, i) => ({ date: addDays(from, i * step), value })),
});

/** A week of data ending today, shaped like the server's answer for that metric. */
const WEEK_SERIES: Record<MetricKey, (from: string) => ReturnType<typeof series>[]> = {
  heart_rate: (f) => [series('resting', f, [54, null, 55, 56, 55, 54, 53]), series('high', f, [110, null, 121, 118, 120, 111, 113])],
  stress: (f) => [series('average', f, [20, null, 30, 25, 28, 22, 26])],
  energy: (f) => [series('high', f, [90, null, 95, 92, 91, 94, 93]), series('low', f, [10, null, 20, 15, 12, 18, 16])],
  steps: (f) => [series('steps', f, [9000, null, 4000, 8000, 10_000, 7000, 8500])],
  intensity_minutes: (f) => [series('moderate', f, [10, null, 15, 0, 20, 5, 30]), series('vigorous', f, [0, null, 5, 0, 5, 0, 10])],
  training_load: (f) => [series('acute', f, [300, 305, 310, 308, 312, 315, 318]), series('chronic', f, [280, 281, 282, 283, 283, 284, 285])],
  vo2_max: (f) => [series('vo2_max', f, [null, null, 49, null, null, null, 50.4])],
  respiration: (f) => [series('asleep', f, [14, null, 15, 14, 14, 15, 14]), series('awake', f, [16, null, 17, 16, 16, 17, 16])],
  spo2: (f) => [series('average', f, [95, null, 96, 95, 96, 95, 96]), series('low', f, [91, null, 93, 92, 92, 91, 93])],
};

function metric(key: MetricKey, range: string, over: Partial<MetricRange> = {}): MetricRange {
  return {
    sample: false,
    metric: key,
    range,
    from: day(6),
    to: TODAY,
    bucket: 'day',
    series: WEEK_SERIES[key](day(6)),
    stats: [],
    goal: null,
    target_low: null,
    target_high: null,
    has_prev: true,
    has_next: false,
    ...over,
  };
}

const emptyDay = (date: string): DayResponse => ({
  sample: false,
  date,
  previous_date: addDays(date, -1),
  next_date: date < TODAY ? addDays(date, 1) : null,
  heart_rate: { resting_hr_bpm: 51, avg_7d_resting_hr_bpm: 52, day: { low_bpm: 45, high_bpm: 148, series: [{ time: `${date}T06:00:00+01:00`, value: 50 }, { time: `${date}T18:00:00+01:00`, value: 148 }] } },
  stress: { average: null, main_level: null, rest_s: null, low_s: null, medium_s: null, high_s: null, activity_s: null, series: [] },
  energy: { now: null, now_at: null, charged: null, drained: null, high: null, low: null, wake_time: null, series: [] },
  steps: 5609,
  step_goal: 8000,
  intensity_minutes: 19,
  breathing: { lowest: 12, highest: 18, avg_awake: 16, avg_asleep: 14, hourly: [{ time: `${date}T08:00:00+01:00`, value: 15 }, { time: `${date}T09:00:00+01:00`, value: 16 }] },
  spo2: { average: 95, lowest: 90, latest: 96, latest_at: `${date}T22:00:00+01:00`, avg_asleep: 94, hourly: [{ time: `${date}T01:00:00+01:00`, value: 94 }, { time: `${date}T02:00:00+01:00`, value: 95 }] },
  sleep: null,
  activities: [],
});

const movement = (date: string): MovementResponse => ({
  sample: false,
  date,
  week_number: 47,
  steps: 5609,
  step_goal: 8000,
  percent_of_goal: 70,
  distance_m: 4200,
  floors: null,
  week_steps: Array.from({ length: 7 }, (_, i) => ({ date: addDays(weekStart(date), i), value: addDays(weekStart(date), i) <= date ? 6000 : null })),
  week_average_steps: 6000,
  intensity: {
    week_start: weekStart(date),
    moderate_min: 30,
    vigorous_min: 10,
    total_min: 50,
    goal_min: 150,
    days: Array.from({ length: 7 }, (_, i) => ({ date: addDays(weekStart(date), i), moderate_min: i === 0 ? 30 : null, vigorous_min: i === 0 ? 10 : null, total_min: i === 0 ? 50 : null })),
  },
  calories: { active_kcal: 400, resting_kcal: 1700, total_kcal: 2100 },
});

/** The label of the arrows: a day, a week for intensity minutes, else a period. */
const arrowUnit = (key: MetricKey, range: string) => (range === '1d' ? 'day' : key === 'intensity_minutes' && range === '7d' ? 'week' : 'period');

function mockApi(routes: Record<string, unknown>) {
  const calls: string[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      calls.push(url);
      const hit = Object.entries(routes).find(([prefix]) => url.startsWith(prefix));
      if (!hit) return new Response(JSON.stringify({ error: `no mock for ${url}` }), { status: 404, headers: { 'content-type': 'application/json' } });
      const [, value] = hit;
      if (typeof value === 'function') return (value as (u: string) => Response)(url);
      return new Response(JSON.stringify(value), { status: 200, headers: { 'content-type': 'application/json' } });
    }),
  );
  return calls;
}

function Where() {
  const loc = useLocation();
  return <output data-testid="where">{loc.pathname + loc.search}</output>;
}

function renderAt(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/metrics/:metric" element={<MetricPage />} />
          <Route path="*" element={<p>elsewhere</p>} />
        </Routes>
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('MetricPage on a wide screen', () => {
  const real = window.matchMedia;
  afterEach(() => {
    window.matchMedia = real;
  });
  const wide = () => {
    window.matchMedia = ((query: string) => ({ matches: query.includes('min-width: 1024px'), media: query, addEventListener: () => {}, removeEventListener: () => {} })) as unknown as typeof window.matchMedia;
  };

  it('has the title, a toolbar with the arrows, the dates and the range labels in words, and the figures in a strip', async () => {
    wide();
    const calls = mockApi({ '/api/metrics/heart_rate': metric('heart_rate', '4w', { stats: [{ key: 'avg_resting', label: 'Avg resting', value: 53, unit: 'bpm' }] as MetricRange['stats'] }) });
    renderAt('/metrics/heart_rate?range=4w');
    expect(await screen.findByRole('heading', { level: 1, name: 'Heart rate' })).toBeInTheDocument();
    const tabs = screen.getByRole('group', { name: 'Period' });
    expect(within(tabs).getAllByRole('button').map((b) => b.textContent)).toEqual(['1 Day', '7 Days', '4 Weeks', '1 Year']);
    expect(within(tabs).getByRole('button', { name: '4 Weeks' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: 'Next period' })).toBeDisabled();
    expect(await screen.findByText('Avg resting')).toBeInTheDocument();
    expect(document.querySelector('.stat-strip')).not.toBeNull();
    expect(document.querySelector('.metric-tiles')).toBeNull();
    expect(calls.some((c) => c.startsWith('/api/metrics/heart_rate?range=4w'))).toBe(true);
    await userEvent.click(within(tabs).getByRole('button', { name: '7 Days' }));
    expect(screen.getByTestId('where')).toHaveTextContent('range=7d');
    await userEvent.click(screen.getByRole('button', { name: 'Previous period' }));
    expect(screen.getByTestId('where')).toHaveTextContent('end=');
  });
});

describe('MetricPage, one day, on a wide screen', () => {
  const real = window.matchMedia;
  afterEach(() => {
    window.matchMedia = real;
  });

  it('puts the figures in a strip in the left column and the chart as a titled section beside it', async () => {
    window.matchMedia = ((query: string) => ({ matches: query.includes('min-width: 1024px'), media: query, addEventListener: () => {}, removeEventListener: () => {} })) as unknown as typeof window.matchMedia;
    mockApi({ [`/api/days/${TODAY}`]: emptyDay(TODAY) });
    renderAt('/metrics/heart_rate');
    const side = await waitFor(() => {
      const el = document.querySelector('.stats-side');
      if (!el) throw new Error('no side column yet');
      return el as HTMLElement;
    });
    expect(within(side).getByRole('heading', { name: 'Summary' })).toBeInTheDocument();
    expect(within(side).getByText('Resting')).toBeInTheDocument();
    expect(within(document.querySelector('.stats-main') as HTMLElement).getByRole('heading', { name: 'Heart rate' })).toBeInTheDocument();
    expect(document.querySelector('.metric-tiles')).toBeNull();
  });
});

describe('MetricPage, a week', () => {
  const keys = Object.keys(METRICS) as MetricKey[];

  it.each(keys.filter((k) => METRICS[k].ranges.includes('7d')))('%s: draws the period and has a row for every day with a value, newest first', async (key) => {
    // Intensity minutes show the calendar week; the others the 7 days up to today.
    const over = key === 'intensity_minutes' ? { from: weekStart(TODAY), to: addDays(weekStart(TODAY), 6), series: WEEK_SERIES[key](weekStart(TODAY)) } : {};
    const calls = mockApi({ [`/api/metrics/${key}`]: metric(key, '7d', over) });
    renderAt(`/metrics/${key}?range=7d`);
    expect(await screen.findByRole('heading', { level: 1, name: METRICS[key].title })).toBeInTheDocument();
    expect(await screen.findByText('Day by day')).toBeInTheDocument();
    expect(calls.some((c) => c === `/api/metrics/${key}?range=7d&end=${TODAY}`)).toBe(true);
    expect(screen.getByRole('button', { name: `Next ${arrowUnit(key, '7d')}` })).toBeDisabled();
    // The second day of every fixture has no value, so it has no row.
    const rows = within(screen.getByRole('region', { name: /newest first/ })).getAllByRole('link');
    expect(rows).toHaveLength(6);
    for (const r of rows) expect(r.getAttribute('href')).toMatch(new RegExp(`^/metrics/${key}\\?range=1d`));
  });

  it('has the tabs of the metric and no tab for the current value', async () => {
    mockApi({ '/api/metrics/spo2': metric('spo2', '7d') });
    renderAt('/metrics/spo2?range=7d');
    await screen.findByText('Day by day');
    const tabs = screen.getByRole('group', { name: 'Period' });
    expect(within(tabs).getAllByRole('button').map((r) => r.textContent)).toEqual(['1d', '7d', '4w']);
  });

  it('steps back and forward by the length of the period, and disables next at today', async () => {
    const calls = mockApi({ '/api/metrics/steps': (u: string) => new Response(JSON.stringify(metric('steps', '7d', { to: new URL(u, 'http://x').searchParams.get('end') ?? TODAY })), { status: 200, headers: { 'content-type': 'application/json' } }) });
    renderAt('/metrics/steps?range=7d');
    await screen.findByText('Day by day');
    expect(screen.getByRole('button', { name: 'Next period' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Previous period' }));
    expect(screen.getByTestId('where')).toHaveTextContent(`/metrics/steps?range=7d&end=${day(7)}`);
    expect(screen.getByRole('button', { name: 'Next period' })).toBeEnabled();
    await screen.findByText('Day by day');
    expect(calls.some((c) => c === `/api/metrics/steps?range=7d&end=${day(7)}`)).toBe(true);
    await userEvent.click(screen.getByRole('button', { name: 'Next period' }));
    expect(screen.getByTestId('where')).toHaveTextContent(/^\/metrics\/steps\?range=7d$/);
  });

  it('switches tab through the address and keeps the period\'s last day', async () => {
    mockApi({ '/api/metrics/steps': metric('steps', '4w') });
    renderAt(`/metrics/steps?range=7d&end=${day(10)}`);
    await screen.findByText('Day by day');
    await userEvent.click(screen.getByRole('button', { name: '4w' }));
    expect(screen.getByTestId('where')).toHaveTextContent(`/metrics/steps?range=4w&end=${day(10)}`);
  });

  it('opens a day row on 1d for that day', async () => {
    mockApi({ '/api/metrics/steps': metric('steps', '7d'), [`/api/days/${day(1)}`]: emptyDay(day(1)) });
    renderAt('/metrics/steps?range=7d');
    await screen.findByText('Day by day');
    const rows = within(screen.getByRole('region', { name: /newest first/ })).getAllByRole('link');
    // Today is the first row; yesterday the second.
    await userEvent.click(rows[1] as HTMLElement);
    expect(screen.getByTestId('where')).toHaveTextContent(`/metrics/steps?range=1d&end=${day(1)}`);
  });
});

describe('MetricPage, weeks', () => {
  it('opens a week row of the year on 7d for that week', async () => {
    const from = day(363);
    const year = metric('steps', '1y', {
      from,
      bucket: 'week',
      series: [series('steps', from, Array.from({ length: 52 }, (_, i) => 40_000 + i), 7)],
    });
    mockApi({ '/api/metrics/steps': year });
    renderAt('/metrics/steps?range=1y');
    expect(await screen.findByText('Week by week')).toBeInTheDocument();
    const rows = within(screen.getByRole('region', { name: /newest first/ })).getAllByRole('link');
    expect(rows).toHaveLength(52);
    await userEvent.click(rows[1] as HTMLElement);
    // The second newest bucket starts 7 days before the last one; it ends six days later.
    const lastStart = addDays(from, 51 * 7);
    expect(screen.getByTestId('where')).toHaveTextContent(`/metrics/steps?range=7d&end=${addDays(addDays(lastStart, -7), 6)}`);
  });

  it('has 4w, 12w and 6m for the VO2 max and no link on its rows', async () => {
    mockApi({ '/api/metrics/vo2_max': metric('vo2_max', '4w', { from: day(27) }) });
    renderAt('/metrics/vo2_max');
    await screen.findByText('Day by day');
    const tabs = screen.getByRole('group', { name: 'Period' });
    expect(within(tabs).getAllByRole('button').map((r) => r.textContent)).toEqual(['4w', '12w', '6m']);
    expect(within(screen.getByRole('region', { name: /newest first/ })).queryAllByRole('link')).toHaveLength(0);
  });
});

describe('MetricPage, a day', () => {
  it('opens on the day, and a heart rate day shows the chart and the activities section', async () => {
    const calls = mockApi({ [`/api/days/${TODAY}`]: emptyDay(TODAY) });
    renderAt('/metrics/heart_rate');
    expect(await screen.findByText('Resting 51 · High 148')).toBeInTheDocument();
    expect(calls.some((c) => c === `/api/days/${TODAY}`)).toBe(true);
    expect(screen.getByRole('button', { name: 'Next day' })).toBeDisabled();
  });

  it.each(['steps', 'intensity_minutes', 'respiration', 'spo2', 'stress', 'energy'] as const)('%s: the 1d tab loads the day', async (key) => {
    mockApi({ [`/api/days/${TODAY}`]: emptyDay(TODAY), '/api/movement/': movement(TODAY) });
    renderAt(`/metrics/${key}?range=1d`);
    expect(await screen.findByRole('heading', { level: 1, name: METRICS[key].title })).toBeInTheDocument();
    expect(await screen.findByRole('button', { name: '1d', pressed: true })).toBeInTheDocument();
  });
});

describe('MetricPage, states', () => {
  it('says so when the period has no data, and still has the tabs and arrows', async () => {
    const none = metric('heart_rate', '7d', { series: [series('resting', day(6), Array(7).fill(null)), series('high', day(6), Array(7).fill(null))] });
    mockApi({ '/api/metrics/heart_rate': none });
    renderAt('/metrics/heart_rate?range=7d');
    expect(await screen.findByText('No heart rate in this period')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '4w' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Previous period' })).toBeEnabled();
    expect(screen.queryByText(/newest first/)).not.toBeInTheDocument();
  });

  it('says so when the request fails, and tries again', async () => {
    let ok = false;
    mockApi({
      '/api/metrics/stress': () => (ok ? new Response(JSON.stringify(metric('stress', '7d')), { status: 200, headers: { 'content-type': 'application/json' } }) : new Response('{"error":"boom"}', { status: 500, headers: { 'content-type': 'application/json' } })),
    });
    renderAt('/metrics/stress?range=7d');
    const retry = await screen.findByRole('button', { name: 'Try again' });
    ok = true;
    await userEvent.click(retry);
    expect(await screen.findByText('Day by day')).toBeInTheDocument();
  });

  it('falls back to the default tab for a tab the metric does not have', async () => {
    mockApi({ '/api/metrics/vo2_max': metric('vo2_max', '4w') });
    renderAt('/metrics/vo2_max?range=1y');
    await screen.findByText('Day by day');
    expect(screen.getByRole('button', { name: '4w', pressed: true })).toBeInTheDocument();
  });

  it('is not found for a name that is not a metric', async () => {
    mockApi({});
    renderAt('/metrics/hrv');
    expect(await screen.findByText(/not found|doesn.t exist|no such/i)).toBeInTheDocument();
  });
});
