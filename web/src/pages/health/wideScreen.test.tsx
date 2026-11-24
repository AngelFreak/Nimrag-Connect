import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { HrvRange } from '../../api/generated/HrvRange';
import type { SleepNightResponse } from '../../api/generated/SleepNightResponse';
import type { SleepNightsResponse } from '../../api/generated/SleepNightsResponse';
import type { TrainingStatusRange } from '../../api/generated/TrainingStatusRange';
import { addDays, browserToday } from '../../lib/dates';
import { DayPage } from '../DayPage';
import { HealthPage } from '../HealthPage';
import { HrvPage } from '../HrvPage';
import { SleepPage } from '../SleepPage';
import { TrainingStatusPage } from '../TrainingStatusPage';

// The stats pages at >= 1024 px: the title and toolbar header, the one-day pages in two columns (a
// 320 px summary and the factors and charts beside it), the figures in strips. The phone layout is
// covered by pages.test.tsx and statusPages.test.tsx, which run without a media query.
vi.mock('../../components/SeriesChart', () => ({
  SeriesChart: ({ label, summary }: { label: string; summary: string }) => (
    <div role="img" aria-label={label}>
      <p className="sr-only">{summary}</p>
    </div>
  ),
}));

const real = window.matchMedia;
beforeEach(() => {
  window.matchMedia = ((query: string) => ({ matches: query.includes('min-width: 1024px'), media: query, addEventListener: () => {}, removeEventListener: () => {} })) as unknown as typeof window.matchMedia;
});
afterEach(() => {
  window.matchMedia = real;
  vi.unstubAllGlobals();
});

const TODAY = browserToday();

function mockApi(routes: Record<string, unknown>) {
  const calls: string[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      calls.push(url);
      const key = Object.keys(routes).find((k) => url.startsWith(k));
      const headers = { 'content-type': 'application/json' };
      return key ? new Response(JSON.stringify(routes[key]), { status: 200, headers }) : new Response('{"error":"none"}', { status: 404, headers });
    }),
  );
  return calls;
}

function Where() {
  const loc = useLocation();
  return <output data-testid="where">{loc.pathname + loc.search}</output>;
}

function renderAt(path: string, ui: React.ReactNode, route: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path={route} element={ui} />
          <Route path="*" element={<Where />} />
        </Routes>
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const NIGHT: SleepNightResponse = {
  sample: false,
  date: TODAY,
  night: {
    date: TODAY,
    start: `${addDays(TODAY, -1)}T23:18:00+01:00`,
    end: `${TODAY}T06:42:00+01:00`,
    duration_s: 25920,
    time_in_bed_s: 26640,
    score: 81,
    score_label: 'good',
    scores: { overall: 81, quality: 79, duration: 92, recovery: null, deep: 85, light: 70, rem: 55 },
    awakenings: 2,
    avg_stress: 21,
    stages_available: true,
    deep_s: 5040,
    light_s: 14280,
    rem_s: 6600,
    awake_s: 720,
    stages: [{ start: `${addDays(TODAY, -1)}T23:18:00+01:00`, end: `${TODAY}T01:00:00+01:00`, stage: 'light' }],
    hrv_ms: 62,
    lowest_hr_bpm: 48,
    breathing_brpm: 13.4,
    spo2_avg_percent: 95,
    spo2_low_percent: 91,
  },
  last_nights: [],
  average_duration_s: 25200,
};
const NIGHTS: SleepNightsResponse = {
  sample: false,
  nights: [{ date: TODAY, start: NIGHT.night!.start, end: NIGHT.night!.end, duration_s: 25920, score: 81, deep_s: null, light_s: null, rem_s: null, awake_s: null }],
  average_duration_s: 25200,
  average_score: 81,
};

describe('Sleep on a wide screen', () => {
  it('has the title, a toolbar with the range in words, the score and its factors with a rating word each', async () => {
    mockApi({ [`/api/sleep/${TODAY}`]: NIGHT, '/api/sleep?nights=': NIGHTS });
    renderAt(`/sleep/${TODAY}`, <SleepPage />, '/sleep/:date?');
    expect(await screen.findByRole('heading', { level: 1, name: 'Sleep' })).toBeInTheDocument();
    const tabs = screen.getByRole('group', { name: 'Period' });
    expect(within(tabs).getAllByRole('button').map((b) => b.textContent)).toEqual(['1 Day', '7 Days', '4 Weeks', '1 Year']);
    expect(await screen.findByRole('heading', { name: 'Sleep Score' })).toBeInTheDocument();
    const factors = within(await screen.findByRole('list', { name: 'Sleep score factors' }));
    const row = (name: string) => factors.getByText(name).closest('.factor-row') as HTMLElement;
    expect(within(row('Duration')).getByText('Excellent')).toBeInTheDocument();
    expect(within(row('Deep')).getByText('Good')).toBeInTheDocument();
    expect(within(row('Light')).getByText('Fair')).toBeInTheDocument();
    expect(within(row('REM')).getByText('Poor')).toBeInTheDocument();
    // No recovery score in this night: the row shows what was measured and no made-up rating.
    expect(within(row('Recovery')).queryByText(/^(Excellent|Good|Fair|Poor)$/)).not.toBeInTheDocument();
    expect(within(row('Recovery')).getByText('21 avg stress')).toBeInTheDocument();
    expect(document.querySelector('.stats-cols')).not.toBeNull();
    expect(document.querySelector('.stat-strip')).not.toBeNull();
  });

  it('steps a night back with the arrow and goes to the latest night with 1 Day', async () => {
    mockApi({ [`/api/sleep/${TODAY}`]: NIGHT, '/api/sleep?nights=': NIGHTS });
    renderAt(`/sleep/${TODAY}`, <SleepPage />, '/sleep/:date?');
    await screen.findByRole('heading', { name: 'Sleep Score' });
    expect(screen.getByRole('button', { name: 'Next night' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Previous night' }));
    expect(screen.getAllByTestId('where')[0]).toHaveTextContent(`/sleep/${addDays(TODAY, -1)}`);
  });
});

describe('HRV status on a wide screen', () => {
  const range: HrvRange = {
    sample: false,
    from: addDays(TODAY, -6),
    to: TODAY,
    today: TODAY,
    days: [
      {
        date: TODAY,
        status_code: 1,
        status: 'balanced',
        status_label: 'Balanced',
        weekly_avg_ms: 64,
        last_night_ms: 66,
        last_night_5min_high_ms: 80,
        baseline_low_ms: 55,
        baseline_high_ms: 70,
      },
    ],
    nights: 1,
    overnight_avg_ms: 66,
    baseline_low_ms: 55,
    baseline_high_ms: 70,
    night: null,
  };

  it('shows the averages chart, a pill that switches to the overnight values, and the figures in a strip', async () => {
    mockApi({ '/api/hrv': range });
    renderAt('/hrv?range=7d', <HrvPage />, '/hrv');
    expect(await screen.findByRole('heading', { level: 1, name: 'HRV Status' })).toBeInTheDocument();
    expect(await screen.findByRole('heading', { name: '7-day averages' })).toBeInTheDocument();
    const pill = screen.getByRole('button', { name: 'Overnight averages' });
    expect(pill).toHaveAttribute('aria-pressed', 'false');
    await userEvent.click(pill);
    expect(pill).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('heading', { name: 'Overnight averages' })).toBeInTheDocument();
    expect(screen.getByText('Baseline')).toBeInTheDocument();
    expect(document.querySelector('.stat-strip')).not.toBeNull();
  });
});

describe('Training status on a wide screen', () => {
  const data: TrainingStatusRange = {
    sample: false,
    from: addDays(TODAY, -6),
    to: TODAY,
    today: TODAY,
    days: [{ date: TODAY, status: { code: 7, label: 'Productive', kind: 'productive' } }],
    runs: [{ status: { code: 7, label: 'Productive', kind: 'productive' }, from: TODAY, to: TODAY, days: 1, starts_before_range: false, ongoing: true }],
    legend: [{ code: 7, label: 'Productive', kind: 'productive' }],
    current: { status_code: 7, label: 'Productive', kind: 'productive', since: addDays(TODAY, -2), since_is_lower_bound: false, fitness_trend_code: null, load_trend_code: null, as_of: `${TODAY}T08:00:00Z` },
    load: { seven_day_load: 300, target_min: 200, target_max: 340, vs_target: 'within', acute: 288, chronic: 244, ratio: 1.2, series_4w: [] },
  };

  it('puts today in a left column with the load figures and the timeline and history beside it', async () => {
    mockApi({ '/api/training/status': data });
    renderAt('/training/status', <TrainingStatusPage />, '/training/status');
    expect(await screen.findByRole('heading', { level: 1, name: 'Training Status' })).toBeInTheDocument();
    expect(await screen.findByRole('heading', { name: 'Status by day' })).toBeInTheDocument();
    const side = document.querySelector('.stats-side') as HTMLElement;
    expect(within(side).getByText('Productive')).toBeInTheDocument();
    expect(within(side).getByText('Acute load')).toBeInTheDocument();
    expect(within(side).getByText('288')).toBeInTheDocument();
    const main = document.querySelector('.stats-main') as HTMLElement;
    expect(within(main).getByRole('heading', { name: 'Status by day' })).toBeInTheDocument();
    expect(within(main).getByRole('table')).toBeInTheDocument();
    const tabs = screen.getByRole('group', { name: 'Period' });
    expect(within(tabs).getAllByRole('button').map((b) => b.textContent)).toEqual(['Current', '4 Weeks', '12 Weeks', '6 Months']);
  });
});

describe('Day and Health on a wide screen', () => {
  it('Day: the date in a chip, arrows that move a day, the figures in the left column and the charts as sections', async () => {
    mockApi({
      '/api/days/': {
        sample: false,
        date: TODAY,
        previous_date: addDays(TODAY, -1),
        next_date: null,
        heart_rate: { resting_hr_bpm: 51, avg_7d_resting_hr_bpm: 52, day: { low_bpm: 45, high_bpm: 148, series: [] } },
        stress: { average: 28, main_level: 'low', rest_s: 0, low_s: 0, medium_s: 0, high_s: 0, activity_s: 0, series: [] },
        energy: { now: 68, now_at: null, charged: 52, drained: 56, high: 94, low: 38, wake_time: null, series: [] },
        steps: 5609,
        step_goal: 8000,
        intensity_minutes: 19,
        breathing: { lowest: null, highest: null, avg_awake: null, avg_asleep: null, hourly: [] },
        spo2: { average: null, lowest: null, latest: null, latest_at: null, avg_asleep: null, hourly: [] },
        sleep: null,
        activities: [],
      },
    });
    renderAt(`/day/${TODAY}`, <DayPage />, '/day/:date?');
    expect(await screen.findByRole('heading', { level: 1, name: 'Day' })).toBeInTheDocument();
    expect(await screen.findByRole('heading', { name: 'Heart rate' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Steps' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Activities' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Next day' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Previous day' }));
    expect(screen.getAllByTestId('where')[0]).toHaveTextContent(`/day/${addDays(TODAY, -1)}`);
  });

  it('Health: the pages to open first, the trends under them', async () => {
    mockApi({ '/api/trends': { error: 'x' } });
    renderAt('/health', <HealthPage />, '/health');
    expect(await screen.findByRole('heading', { level: 1, name: 'Health' })).toBeInTheDocument();
    const explore = screen.getByRole('heading', { name: 'Explore' }).closest('section') as HTMLElement;
    expect(explore.querySelector('a[href="/sleep"]')).not.toBeNull();
    expect(screen.getByRole('heading', { name: 'Trends' })).toBeInTheDocument();
  });
});
