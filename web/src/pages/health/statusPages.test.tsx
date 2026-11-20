import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HrvDay } from '../../api/generated/HrvDay';
import type { HrvRange } from '../../api/generated/HrvRange';
import type { SleepBucket } from '../../api/generated/SleepBucket';
import type { SleepHistory } from '../../api/generated/SleepHistory';
import type { StatusInfo } from '../../api/generated/StatusInfo';
import type { TrainingStatusRange } from '../../api/generated/TrainingStatusRange';
import { addDays, browserToday } from '../../lib/dates';
import { HrvPage } from '../HrvPage';
import { SleepPage } from '../SleepPage';
import { TrainingStatusPage } from '../TrainingStatusPage';

// Page tests for the three range pages (Training status, HRV status, Sleep score 7d/4w/1y): what they
// show with no data, one point, a few points and gaps, and that the period control and arrows work.
// uPlot needs a real canvas, so the chart is replaced by its name and text alternative.
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

const info = (label: string, kind: StatusInfo['kind'], code: number | null): StatusInfo => ({ code, label, kind });
const RECOVERY = info('Recovery', 'recovery', 5);
const STRAINED = info('Strained', 'strained', 8);
const MAINTAINING = info('Maintaining', 'maintaining', 4);
const NO_STATUS = info('No status', 'none', 0);
const LEGEND = [RECOVERY, STRAINED, MAINTAINING, info('Productive', 'productive', 7), info('Detraining', 'detraining', 1), NO_STATUS];

const EMPTY_CURRENT: TrainingStatusRange['current'] = {
  status_code: null,
  label: null,
  kind: null,
  since: null,
  since_is_lower_bound: false,
  fitness_trend_code: null,
  load_trend_code: null,
  as_of: null,
};
const NO_LOAD: TrainingStatusRange['load'] = { seven_day_load: null, target_min: null, target_max: null, vs_target: null, acute: null, chronic: null, ratio: null, series_4w: [] };

function training(over: Partial<TrainingStatusRange> = {}): TrainingStatusRange {
  return { sample: false, from: day(6), to: TODAY, today: TODAY, days: [], runs: [], legend: LEGEND, current: EMPTY_CURRENT, load: NO_LOAD, ...over };
}

function hrv(over: Partial<HrvRange> = {}): HrvRange {
  return { sample: false, from: day(6), to: TODAY, today: TODAY, days: [], nights: 0, overnight_avg_ms: null, baseline_low_ms: null, baseline_high_ms: null, night: null, ...over };
}
const hrvDay = (date: string, over: Partial<HrvDay> = {}): HrvDay => ({
  date,
  status_code: null,
  status: null,
  status_label: null,
  weekly_avg_ms: null,
  last_night_ms: null,
  last_night_5min_high_ms: null,
  baseline_low_ms: null,
  baseline_high_ms: null,
  ...over,
});

const bucket = (from: string, over: Partial<SleepBucket> = {}): SleepBucket => ({
  from,
  to: from,
  nights: 1,
  avg_score: 80,
  high_score: 80,
  low_score: 80,
  avg_duration_s: 8 * 3600,
  ...over,
});
function history(buckets: SleepBucket[], over: Partial<SleepHistory['stats']> = {}, group = 'day'): SleepHistory {
  const nights = buckets.filter((b) => b.nights > 0).length;
  return {
    sample: false,
    from: buckets[0]?.from ?? day(6),
    to: buckets[buckets.length - 1]?.to ?? TODAY,
    today: TODAY,
    group,
    long_night_s: 8 * 3600,
    buckets,
    stats: {
      nights,
      scored_nights: nights,
      avg_score: nights ? 80 : null,
      highest_score: nights ? 80 : null,
      lowest_score: nights ? 80 : null,
      avg_highest: null,
      avg_lowest: null,
      avg_duration_s: nights ? 8 * 3600 : null,
      long_nights: nights,
      ...over,
    },
  };
}

function mockApi(routes: Record<string, unknown>) {
  const calls: string[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      calls.push(url);
      const hit = Object.entries(routes).find(([prefix]) => url.startsWith(prefix));
      if (!hit) return new Response(JSON.stringify({ error: `no mock for ${url}` }), { status: 404, headers: { 'content-type': 'application/json' } });
      return new Response(JSON.stringify(hit[1]), { status: 200, headers: { 'content-type': 'application/json' } });
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
          <Route path="/training/status" element={<TrainingStatusPage />} />
          <Route path="/hrv" element={<HrvPage />} />
          <Route path="/sleep/:date?" element={<SleepPage />} />
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

describe('TrainingStatusPage', () => {
  const filled = training({
    days: [
      { date: day(3), status: STRAINED },
      { date: day(2), status: STRAINED },
      { date: day(1), status: RECOVERY },
      { date: TODAY, status: RECOVERY },
    ],
    runs: [
      { status: RECOVERY, from: day(1), to: TODAY, days: 2, starts_before_range: false, ongoing: true },
      { status: STRAINED, from: day(3), to: day(2), days: 2, starts_before_range: false, ongoing: false },
    ],
    current: { ...EMPTY_CURRENT, status_code: 5, label: 'Recovery', kind: 'recovery', since: day(1), as_of: `${TODAY}T08:00:00+01:00` },
    load: { ...NO_LOAD, acute: 400, chronic: 520, ratio: 0.77 },
  });

  it('shows today big with the load figures, the timeline and the list of runs, newest first', async () => {
    const calls = mockApi({ '/api/training/status': filled });
    renderAt('/training/status');
    expect(await screen.findByText('Training status now')).toBeInTheDocument();
    expect(screen.getByText('400')).toBeInTheDocument();
    expect(screen.getByText('0.77')).toBeInTheDocument();
    expect(calls.some((c) => c === `/api/training/status?from=${day(6)}&to=${TODAY}`)).toBe(true);
    const timeline = screen.getByRole('img', { name: /Training status/ });
    expect(timeline).toHaveAccessibleName(/Recovery since .*Strained/);
    const rows = within(screen.getByRole('table')).getAllByRole('row');
    expect(within(rows[1] as HTMLElement).getByText('Recovery')).toBeInTheDocument();
    expect(within(rows[1] as HTMLElement).getByText(/^Since /)).toBeInTheDocument();
    expect(within(rows[2] as HTMLElement).getByText('Strained')).toBeInTheDocument();
    // the legend names every colour
    for (const s of LEGEND) expect(screen.getAllByText(s.label).length).toBeGreaterThan(0);
  });

  it('has a segmented control, and the forward arrow is disabled at today while the back arrow steps one period', async () => {
    mockApi({ '/api/training/status': filled });
    renderAt('/training/status');
    await screen.findByText('Training status now');
    expect(screen.getByRole('button', { name: 'Next week' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Previous week' }));
    expect(screen.getByTestId('where')).toHaveTextContent(`/training/status?range=current&end=${day(7)}`);
    expect(screen.getByRole('button', { name: 'Next week' })).toBeEnabled();
    await userEvent.click(screen.getByRole('button', { name: '4w' }));
    expect(screen.getByTestId('where')).toHaveTextContent('/training/status?range=4w');
  });

  it('shows an empty page that still explains itself when there is no status at all', async () => {
    mockApi({ '/api/training/status': training() });
    renderAt('/training/status');
    expect(await screen.findByText('No training status has been recorded yet.')).toBeInTheDocument();
    expect(screen.getByText(/No status recorded|no status/i)).toBeInTheDocument();
  });

  it('shows a single day and an unknown status in grey with its number', async () => {
    const unknown = info('Status 9', 'unknown', 9);
    mockApi({
      '/api/training/status': training({
        days: [{ date: TODAY, status: unknown }],
        runs: [{ status: unknown, from: TODAY, to: TODAY, days: 1, starts_before_range: true, ongoing: true }],
        current: { ...EMPTY_CURRENT, status_code: 9, label: 'Status 9', kind: 'unknown', since: TODAY, since_is_lower_bound: true },
      }),
    });
    renderAt('/training/status?range=4w');
    expect((await screen.findAllByText('Status 9')).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/or earlier/).length).toBeGreaterThan(0);
  });

  it('says so when the request fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{"error":"boom"}', { status: 500, headers: { 'content-type': 'application/json' } })),
    );
    renderAt('/training/status');
    expect(await screen.findByRole('button', { name: /try again|retry/i })).toBeInTheDocument();
  });
});

describe('HrvPage', () => {
  const days = [
    hrvDay(day(3), { status: 'balanced', status_code: 4, status_label: 'Balanced', weekly_avg_ms: 61, last_night_ms: 58, baseline_low_ms: 55, baseline_high_ms: 70 }),
    hrvDay(day(2)),
    hrvDay(day(1), { status: 'unbalanced', status_code: 3, status_label: 'Unbalanced', weekly_avg_ms: 52, last_night_ms: 49, baseline_low_ms: 55, baseline_high_ms: 70 }),
  ];

  it('shows the chart, the figures and a card per day with data, newest first', async () => {
    mockApi({ '/api/hrv': hrv({ days, nights: 2, overnight_avg_ms: 53.5, baseline_low_ms: 55, baseline_high_ms: 70 }) });
    renderAt('/hrv');
    expect(await screen.findByRole('img', { name: '7-day average HRV by day' })).toBeInTheDocument();
    expect(screen.getByText('55–70')).toBeInTheDocument();
    expect(screen.getByText('2 nights')).toBeInTheDocument();
    const cards = within(screen.getByRole('region', { name: 'Day by day' })).getAllByRole('listitem');
    expect(cards).toHaveLength(2);
    expect(within(cards[0] as HTMLElement).getByText('Unbalanced')).toBeInTheDocument();
    expect(within(cards[1] as HTMLElement).getByText('Balanced')).toBeInTheDocument();
    expect(within(cards[1] as HTMLElement).getByText('61 ms')).toBeInTheDocument();
    expect(within(cards[1] as HTMLElement).getByText('58 ms')).toBeInTheDocument();
  });

  it('switches the chart to the overnight averages with the toggle', async () => {
    mockApi({ '/api/hrv': hrv({ days, nights: 2, overnight_avg_ms: 53.5 }) });
    renderAt('/hrv');
    await screen.findByRole('img', { name: '7-day average HRV by day' });
    await userEvent.click(screen.getByRole('checkbox', { name: 'Overnight averages' }));
    expect(screen.getByRole('img', { name: 'Overnight HRV by day' })).toBeInTheDocument();
  });

  it('shows an empty state with no readings and no chart claim', async () => {
    mockApi({ '/api/hrv': hrv({ days: [hrvDay(day(1)), hrvDay(TODAY)] }) });
    renderAt('/hrv?range=4w');
    expect(await screen.findByText('No HRV in this period')).toBeInTheDocument();
    expect(within(screen.getByRole('region', { name: 'Day by day' })).queryByRole('listitem')).not.toBeInTheDocument();
    expect(screen.getByText(/No 7-day average HRV readings/)).toBeInTheDocument();
  });

  it('shows one reading', async () => {
    mockApi({
      '/api/hrv': hrv({ days: [hrvDay(TODAY, { status: 'low', status_code: 2, status_label: 'Low', weekly_avg_ms: 38, last_night_ms: 36 })], nights: 1, overnight_avg_ms: 36 }),
    });
    renderAt('/hrv');
    expect(await screen.findByText('1 night')).toBeInTheDocument();
    expect(within(screen.getByRole('region', { name: 'Day by day' })).getAllByRole('listitem')).toHaveLength(1);
    expect(screen.getByText('Low')).toBeInTheDocument();
  });

  it('shows last night with its readings for 1d', async () => {
    const night: HrvRange['night'] = {
      readings: [
        { time: `${day(1)}T23:30:00+01:00`, value: 55 },
        { time: `${TODAY}T01:30:00+01:00`, value: 61 },
      ],
      avg_ms: 58,
      low_ms: 55,
      high_ms: 61,
    };
    mockApi({
      '/api/hrv': hrv({
        from: TODAY,
        to: TODAY,
        days: [hrvDay(TODAY, { status: 'balanced', status_code: 4, status_label: 'Balanced', last_night_ms: 58, weekly_avg_ms: 60, baseline_low_ms: 55, baseline_high_ms: 70 })],
        nights: 1,
        night,
      }),
    });
    renderAt('/hrv?range=1d');
    expect(await screen.findByRole('img', { name: 'HRV readings through the night' })).toBeInTheDocument();
    expect(screen.getByText('Highest')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Previous day' })).toBeInTheDocument();
  });

  it('says so when a night has nothing', async () => {
    mockApi({ '/api/hrv': hrv({ from: TODAY, to: TODAY, days: [hrvDay(TODAY)] }) });
    renderAt('/hrv?range=1d');
    expect(await screen.findByText('No HRV for this night')).toBeInTheDocument();
  });
});

describe('SleepPage for more than one night', () => {
  const week = Array.from({ length: 7 }, (_, i) =>
    bucket(
      day(6 - i),
      i === 2 ? { nights: 0, avg_score: null, high_score: null, low_score: null, avg_duration_s: null } : { avg_score: 70 + i, avg_duration_s: (6.5 + i / 4) * 3600 },
    ),
  );

  it('shows the score, the stats, the duration bars and the table behind them for 7d', async () => {
    const calls = mockApi({
      '/api/sleep/history': history(week, { avg_score: 74, highest_score: 76, lowest_score: 70, scored_nights: 6, long_nights: 2, avg_duration_s: 7.4 * 3600 }),
    });
    renderAt('/sleep?range=7d');
    expect(await screen.findByRole('img', { name: 'Sleep score by night' })).toBeInTheDocument();
    expect(screen.getByText('Avg score')).toBeInTheDocument();
    expect(screen.getByText('Highest')).toBeInTheDocument();
    expect(screen.getByText('Lowest')).toBeInTheDocument();
    expect(screen.getByText('Sleep duration')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Time asleep per night' })).toBeInTheDocument();
    expect(calls.some((c) => c === `/api/sleep/history?from=${day(6)}&to=${TODAY}&group=day`)).toBe(true);
    // the green and blue bars are explained
    expect(screen.getByText('8 h or more')).toBeInTheDocument();
    expect(screen.getByText('Under 8 h')).toBeInTheDocument();
  });

  it('asks for months for 1y and calls the stats averages', async () => {
    const calls = mockApi({
      '/api/sleep/history': history(
        [bucket(day(60), { from: day(60), to: day(30), nights: 20 }), bucket(day(29), { to: TODAY, nights: 25, avg_score: 82 })],
        { avg_highest: 90, avg_lowest: 60 },
        'month',
      ),
    });
    renderAt('/sleep?range=1y');
    await screen.findByRole('img', { name: 'Sleep score by month' });
    expect(screen.getByText('Avg highest')).toBeInTheDocument();
    expect(screen.getByText('Avg lowest')).toBeInTheDocument();
    expect(calls.some((c) => c.endsWith('&group=month'))).toBe(true);
  });

  it('shows an empty state with no nights', async () => {
    mockApi({ '/api/sleep/history': history(week.map((b) => ({ ...b, nights: 0, avg_score: null, high_score: null, low_score: null, avg_duration_s: null }))) });
    renderAt('/sleep?range=4w');
    expect(await screen.findByText('No sleep recorded in this period')).toBeInTheDocument();
    expect(screen.queryByRole('img', { name: 'Sleep score by night' })).not.toBeInTheDocument();
  });

  it('shows one night', async () => {
    mockApi({ '/api/sleep/history': history([bucket(TODAY, { avg_score: 77, avg_duration_s: 7 * 3600 })], { long_nights: 0, avg_duration_s: 7 * 3600 }) });
    renderAt('/sleep?range=7d');
    expect(await screen.findByRole('img', { name: 'Sleep score by night' })).toBeInTheDocument();
    expect(screen.getByText('1 scored night')).toBeInTheDocument();
  });

  it('goes back to the night view with 1d', async () => {
    mockApi({ '/api/sleep/history': history(week) });
    renderAt('/sleep?range=7d');
    await screen.findByRole('img', { name: 'Sleep score by night' });
    await userEvent.click(screen.getByRole('button', { name: '1d' }));
    expect(screen.getByTestId('where')).toHaveTextContent(/^\/sleep$/);
  });
});
