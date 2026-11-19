import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DayResponse } from '../../api/generated/DayResponse';
import type { NightSummary } from '../../api/generated/NightSummary';
import type { SleepNightResponse } from '../../api/generated/SleepNightResponse';
import type { SleepNightsResponse } from '../../api/generated/SleepNightsResponse';
import type { TrendSeries } from '../../api/generated/TrendSeries';
import type { TrendsResponse } from '../../api/generated/TrendsResponse';
import { DayPage } from '../DayPage';
import { HealthPage } from '../HealthPage';
import { SleepPage } from '../SleepPage';

// uPlot needs a real canvas; the page tests only care that the chart gets its name and summary.
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

// ---- fixtures -----------------------------------------------------------------------------------

const summary = (date: string, hours: number): NightSummary => ({
  date,
  start: `${date}T23:00:00+01:00`,
  end: `${date}T07:00:00+01:00`,
  duration_s: hours * 3600,
  score: 80,
  deep_s: null,
  light_s: null,
  rem_s: null,
  awake_s: null,
});

const NIGHTS = [summary('2026-11-16', 7), summary('2026-11-17', 6.5), summary('2026-11-18', 7.5)];

const nightResponse = (over: Partial<NonNullable<SleepNightResponse['night']>> | null = {}): SleepNightResponse => ({
  sample: true,
  date: '2026-11-18',
  night:
    over === null
      ? null
      : {
          date: '2026-11-18',
          start: '2026-11-18T23:18:00+01:00',
          end: '2026-11-19T06:42:00+01:00',
          duration_s: 25920,
          time_in_bed_s: 26640,
          score: 81,
          score_label: 'good',
          scores: { overall: 81, quality: 79, duration: 90, recovery: null, deep: null, light: null, rem: null },
          awakenings: 2,
          avg_stress: 21,
          stages_available: true,
          deep_s: 5040,
          light_s: 14280,
          rem_s: 6600,
          awake_s: 720,
          stages: [
            { start: '2026-11-18T23:18:00+01:00', end: '2026-11-19T01:00:00+01:00', stage: 'light' },
            { start: '2026-11-19T01:00:00+01:00', end: '2026-11-19T02:00:00+01:00', stage: 'deep' },
            { start: '2026-11-19T02:00:00+01:00', end: '2026-11-19T06:42:00+01:00', stage: 'rem' },
          ],
          hrv_ms: 62,
          lowest_hr_bpm: 48,
          breathing_brpm: 13.4,
          spo2_avg_percent: null,
          spo2_low_percent: null,
          ...over,
        },
  last_nights: NIGHTS,
  average_duration_s: 25200,
});

const nightsResponse = (_nights: number): SleepNightsResponse => ({ sample: true, nights: NIGHTS, average_duration_s: 25200, average_score: 80 });

const trendSeries = (values: (number | null)[]): TrendSeries => {
  const nums = values.filter((v): v is number => v !== null);
  return {
    average: nums.length ? nums.reduce((a, b) => a + b, 0) / nums.length : null,
    min: nums.length ? Math.min(...nums) : null,
    max: nums.length ? Math.max(...nums) : null,
    points: values.map((value, i) => ({ date: `2026-11-${String(10 + i).padStart(2, '0')}`, value })),
  };
};

const trendsResponse = (range = '4w', empty = false): TrendsResponse => ({
  sample: true,
  range,
  from: '2026-11-10',
  to: '2026-11-19',
  hrv: { series: trendSeries(empty ? [] : [50, 60, 62, 64, 66]), baseline_low_ms: empty ? null : 55, baseline_high_ms: empty ? null : 70 },
  resting_hr: trendSeries(empty ? [] : [50, 51, 52]),
  breathing: { asleep: trendSeries(empty ? [] : [13, 14]), awake_avg_brpm: empty ? null : 15.2 },
  spo2: { overnight: trendSeries(empty ? [] : [95, 96]), low_percent: empty ? null : 91 },
  sleep_score: trendSeries(empty ? [] : [70, 80]),
  sleep_duration_s: trendSeries(empty ? [] : [25200, 27000]),
});

const dayResponse = (over: Partial<DayResponse> = {}): DayResponse => ({
  sample: true,
  date: '2026-11-18',
  previous_date: '2026-11-17',
  next_date: '2026-11-19',
  heart_rate: {
    resting_hr_bpm: 51,
    avg_7d_resting_hr_bpm: 52,
    day: {
      low_bpm: 45,
      high_bpm: 148,
      series: [
        { time: '2026-11-18T06:00:00+01:00', value: 50 },
        { time: '2026-11-18T18:00:00+01:00', value: 148 },
      ],
    },
  },
  stress: {
    average: 28,
    main_level: 'low',
    rest_s: 33000,
    low_s: 37800,
    medium_s: 7800,
    high_s: 0,
    activity_s: 3000,
    series: [{ time: '2026-11-18T10:00:00+01:00', stress: 30, level: 'low' }],
  },
  energy: {
    now: 68,
    now_at: '2026-11-18T23:55:00+01:00',
    charged: 52,
    drained: 56,
    high: 94,
    low: 38,
    wake_time: '2026-11-18T06:42:00+01:00',
    series: [
      { time: '2026-11-18T06:00:00+01:00', value: 90 },
      { time: '2026-11-18T18:00:00+01:00', value: 40 },
    ],
  },
  steps: 5609,
  step_goal: 8000,
  intensity_minutes: 19,
  sleep: summary('2026-11-18', 7.5),
  activities: [
    {
      id: 'abc',
      kind: 'strength',
      sport_name: 'training',
      sub_sport_name: null,
      start: '2026-11-18T17:49:54+01:00',
      date: '2026-11-18',
      duration_s: 2520,
      distance_m: null,
      avg_pace_s_per_km: null,
      avg_hr_bpm: 112,
      max_hr_bpm: 139,
      calories_kcal: null,
      ascent_m: null,
      sets: 18,
      volume_kg: 3300,
      exercises: 6,
    },
  ],
  ...over,
});

type ApiRoutes = Record<string, unknown>;

function mockApi(routes: ApiRoutes) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    const hit = Object.entries(routes).find(([k]) => url === k);
    if (!hit) return new Response(JSON.stringify({ error: `no mock for ${url}` }), { status: 404, headers: { 'content-type': 'application/json' } });
    const [, value] = hit;
    if (typeof value === 'function') return (value as () => Response)();
    return new Response(JSON.stringify(value), { status: 200, headers: { 'content-type': 'application/json' } });
  });
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
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
          <Route path="/sleep/:date?" element={<SleepPage />} />
          <Route path="/day/:date?" element={<DayPage />} />
          <Route path="/health" element={<HealthPage />} />
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

// ---- Sleep ---------------------------------------------------------------------------------------

describe('SleepPage', () => {
  it('shows the night: time asleep, score with label, stage totals, vitals', async () => {
    mockApi({ '/api/sleep/2026-11-18': nightResponse(), '/api/sleep?nights=7': nightsResponse(7) });
    renderAt('/sleep/2026-11-18');
    expect(await screen.findByText('7h 12m')).toBeInTheDocument();
    expect(screen.getByText('23:18 to 06:42')).toBeInTheDocument();
    expect(screen.getByText('81')).toBeInTheDocument();
    expect(screen.getByText('Good')).toBeInTheDocument();
    expect(screen.getByText('Night of Wed 18 Nov')).toBeInTheDocument();
    // legend: deep 1h 24m, light 3h 58m, REM 1h 50m, awake 12m
    expect(screen.getByText('1h 24m')).toBeInTheDocument();
    expect(screen.getByText('3h 58m')).toBeInTheDocument();
    expect(screen.getByText('1h 50m')).toBeInTheDocument();
    expect(screen.getByText('12m')).toBeInTheDocument();
    expect(screen.getByText('62 ms')).toBeInTheDocument();
    expect(screen.getByText('48 bpm')).toBeInTheDocument();
    expect(screen.getByText('13.4 /min')).toBeInTheDocument();
    // the stage chart has a name and a one-line summary
    expect(screen.getByRole('img', { name: 'Sleep stages through the night' })).toBeInTheDocument();
    expect(screen.getByText(/^Sleep stages through the night: deep sleep 1h 24m/)).toBeInTheDocument();
    // blood oxygen is not available this night, so there is no tile for it
    expect(screen.queryByText('Blood oxygen')).not.toBeInTheDocument();
    expect(document.title).toContain('Sleep');
  });

  it('shows the blood oxygen tile only when the watch recorded it', async () => {
    mockApi({ '/api/sleep/2026-11-18': nightResponse({ spo2_avg_percent: 95.4, spo2_low_percent: 91 }), '/api/sleep?nights=7': nightsResponse(7) });
    renderAt('/sleep/2026-11-18');
    expect(await screen.findByText('95%')).toBeInTheDocument();
    expect(screen.getByText('Low 91%')).toBeInTheDocument();
  });

  it('shows "unavailable" for missing vitals, scores and stages, never 0', async () => {
    mockApi({
      '/api/sleep/2026-11-18': nightResponse({
        score: null,
        score_label: null,
        hrv_ms: null,
        lowest_hr_bpm: null,
        breathing_brpm: null,
        stages_available: false,
        stages: [],
        deep_s: null,
        light_s: null,
        rem_s: null,
        awake_s: null,
        awakenings: null,
      }),
      '/api/sleep?nights=7': nightsResponse(7),
    });
    renderAt('/sleep/2026-11-18');
    await screen.findByText('7h 12m');
    expect(screen.getAllByText('unavailable').length).toBeGreaterThanOrEqual(8);
    expect(screen.getByText('Stages were not recorded for this night.')).toBeInTheDocument();
    expect(screen.queryByText(/^0 ?(ms|bpm|\/min)$/)).not.toBeInTheDocument();
  });

  it('says so when no sleep was recorded that night, and still shows the last nights', async () => {
    mockApi({ '/api/sleep/2026-11-18': nightResponse(null), '/api/sleep?nights=7': nightsResponse(7) });
    renderAt('/sleep/2026-11-18');
    expect(await screen.findByText('No sleep recorded for this night')).toBeInTheDocument();
    expect(await screen.findByText(/Sleep averaged 7h 0m over 3 of the last 7 nights/)).toBeInTheDocument();
  });

  it('links to the previous and next night through the URL; no next after the latest night', async () => {
    mockApi({ '/api/sleep/2026-11-17': { ...nightResponse(), date: '2026-11-17' }, '/api/sleep?nights=7': nightsResponse(7) });
    renderAt('/sleep/2026-11-17');
    await screen.findByText('7h 12m');
    expect(screen.getByRole('link', { name: 'Previous night' })).toHaveAttribute('href', '/sleep/2026-11-16');
    await waitFor(() => expect(screen.getByRole('link', { name: 'Next night' })).toHaveAttribute('href', '/sleep/2026-11-18'));
  });

  it('has no next link on the latest night', async () => {
    mockApi({ '/api/sleep/2026-11-18': nightResponse(), '/api/sleep?nights=7': nightsResponse(7) });
    renderAt('/sleep/2026-11-18');
    await screen.findByText('7h 12m');
    await waitFor(() => expect(screen.queryByRole('link', { name: 'Next night' })).not.toBeInTheDocument());
    expect(screen.getByRole('link', { name: 'Previous night' })).toBeInTheDocument();
  });

  it('opens the latest night when no date is in the URL', async () => {
    const fetchMock = mockApi({ '/api/sleep?nights=7': nightsResponse(7), '/api/sleep/2026-11-18': nightResponse() });
    renderAt('/sleep');
    await screen.findByText('7h 12m');
    expect(screen.getByTestId('where')).toHaveTextContent('/sleep/2026-11-18');
    expect(fetchMock).toHaveBeenCalledWith('/api/sleep/2026-11-18', expect.anything());
  });

  it('asks the API for 14 and 30 nights when the control changes, and labels the chart', async () => {
    const fetchMock = mockApi({
      '/api/sleep/2026-11-18': nightResponse(),
      '/api/sleep?nights=7': nightsResponse(7),
      '/api/sleep?nights=14': nightsResponse(14),
      '/api/sleep?nights=30': nightsResponse(30),
    });
    const user = userEvent.setup();
    renderAt('/sleep/2026-11-18');
    await screen.findByText('7h 12m');
    expect(screen.getByRole('img', { name: 'Sleep duration for the last 7 nights' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '14 nights' }));
    expect(await screen.findByRole('img', { name: 'Sleep duration for the last 14 nights' })).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith('/api/sleep?nights=14', expect.anything());
    expect(screen.getByRole('button', { name: '14 nights' })).toHaveAttribute('aria-pressed', 'true');
    await user.click(screen.getByRole('button', { name: '30 nights' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/sleep?nights=30', expect.anything()));
  });

  it('shows an error with a retry when the night cannot be loaded', async () => {
    let fail = true;
    mockApi({
      '/api/sleep/2026-11-18': () =>
        fail
          ? new Response(JSON.stringify({ error: 'database busy' }), { status: 500, headers: { 'content-type': 'application/json' } })
          : new Response(JSON.stringify(nightResponse()), { headers: { 'content-type': 'application/json' } }),
      '/api/sleep?nights=7': nightsResponse(7),
    });
    const user = userEvent.setup();
    renderAt('/sleep/2026-11-18');
    const alert = await screen.findByRole('alert', {}, { timeout: 3000 });
    expect(within(alert).getByText('Could not load this night')).toBeInTheDocument();
    fail = false;
    await user.click(within(alert).getByRole('button', { name: 'Try again' }));
    expect(await screen.findByText('7h 12m')).toBeInTheDocument();
  });
});

// ---- Day -----------------------------------------------------------------------------------------

describe('DayPage', () => {
  it('shows heart rate, stress, energy, steps, the night and the activities', async () => {
    mockApi({ '/api/days/2026-11-18': dayResponse() });
    renderAt('/day/2026-11-18');
    expect(await screen.findByText('Resting 51 · High 148')).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 1, name: 'Wed 18 Nov' })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Heart rate across the day' })).toBeInTheDocument();
    expect(screen.getByText('Heart rate across the day: resting 51 bpm, ranging from 45 to 148 bpm.', { selector: '.sr-only' })).toBeInTheDocument();
    expect(screen.getByText('Average 28 · mostly low')).toBeInTheDocument();
    expect(screen.getByText('Rest 9h 10m')).toBeInTheDocument();
    expect(screen.getByText('High 94 · Low 38')).toBeInTheDocument();
    expect(screen.getByText('+52 charged')).toBeInTheDocument();
    expect(screen.getByText('-56 drained')).toBeInTheDocument();
    expect(screen.getByText('5,609')).toBeInTheDocument();
    expect(screen.getByText('70% of 8,000')).toBeInTheDocument();
    expect(screen.getByText('19 min')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Sleep/ })).toHaveAttribute('href', '/sleep/2026-11-18');
    expect(screen.getByRole('link', { name: /Strength/ })).toHaveAttribute('href', '/activities/abc');
  });

  it('takes previous and next day from the response', async () => {
    mockApi({ '/api/days/2026-11-18': dayResponse({ previous_date: '2026-11-16', next_date: '2026-11-20' }) });
    renderAt('/day/2026-11-18');
    await screen.findByText('Resting 51 · High 148');
    expect(screen.getByRole('link', { name: 'Previous day' })).toHaveAttribute('href', '/day/2026-11-16');
    expect(screen.getByRole('link', { name: 'Next day' })).toHaveAttribute('href', '/day/2026-11-20');
  });

  it('has no next link on the last day', async () => {
    mockApi({ '/api/days/2026-11-19': dayResponse({ date: '2026-11-19', next_date: null }) });
    renderAt('/day/2026-11-19');
    await screen.findByText('Resting 51 · High 148');
    expect(screen.queryByRole('link', { name: 'Next day' })).not.toBeInTheDocument();
  });

  it('shows "unavailable" for missing data and says when there are no activities', async () => {
    mockApi({
      '/api/days/2026-11-18': dayResponse({
        heart_rate: { resting_hr_bpm: null, avg_7d_resting_hr_bpm: null, day: { low_bpm: null, high_bpm: null, series: [] } },
        stress: { average: null, main_level: null, rest_s: null, low_s: null, medium_s: null, high_s: null, activity_s: null, series: [] },
        energy: { now: null, now_at: null, charged: null, drained: null, high: null, low: null, wake_time: null, series: [] },
        steps: null,
        intensity_minutes: null,
        sleep: null,
        activities: [],
      }),
    });
    renderAt('/day/2026-11-18');
    expect(await screen.findByText('No activities this day')).toBeInTheDocument();
    expect(screen.getAllByText('unavailable').length).toBeGreaterThanOrEqual(10);
    expect(screen.queryByText('0')).not.toBeInTheDocument();
    expect(screen.queryByRole('img', { name: 'Heart rate across the day' })).not.toBeInTheDocument();
  });

  it('shows an error state with a retry', async () => {
    mockApi({ '/api/days/2026-11-18': () => new Response('', { status: 500 }) });
    renderAt('/day/2026-11-18');
    expect(await screen.findByRole('button', { name: 'Try again' }, { timeout: 3000 })).toBeInTheDocument();
  });
});

// ---- Health --------------------------------------------------------------------------------------

describe('HealthPage', () => {
  it('shows each trend with its summary line and links to the other pages', async () => {
    mockApi({ '/api/trends?range=4w': trendsResponse() });
    renderAt('/health');
    expect(await screen.findByText('Avg 60 ms · range 55 to 70')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Overnight HRV over 4 weeks' })).toBeInTheDocument();
    expect(screen.getByText('HRV averaged 60 ms, within your range on 4 of 5 nights.', { selector: '.sr-only' })).toBeInTheDocument();
    expect(screen.getByText('Avg 51 bpm · 50 to 52')).toBeInTheDocument();
    expect(screen.getByText('13.5 /min')).toBeInTheDocument();
    expect(screen.getByText('Asleep · awake 15.2')).toBeInTheDocument();
    expect(screen.getByText('96%')).toBeInTheDocument(); // 95.5 rounds up
    expect(screen.getByText('Overnight avg · low 91%')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Sleep score over 4 weeks' })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Sleep duration over 4 weeks' })).toBeInTheDocument();
    for (const [name, href] of [
      ['Sleep', '/sleep'],
      ['Day', '/day'],
      ['Movement', '/movement'],
      ['Training', '/training'],
      ['Body', '/body'],
      ['Calendar', '/calendar'],
    ]) {
      expect(screen.getByRole('link', { name: new RegExp(`^${name}`) })).toHaveAttribute('href', href);
    }
  });

  it('requests the chosen range and marks it pressed', async () => {
    const fetchMock = mockApi({ '/api/trends?range=4w': trendsResponse('4w'), '/api/trends?range=7d': trendsResponse('7d'), '/api/trends?range=1y': trendsResponse('1y') });
    const user = userEvent.setup();
    renderAt('/health');
    await screen.findByText('Avg 60 ms · range 55 to 70');
    expect(screen.getByRole('button', { name: '4 weeks' })).toHaveAttribute('aria-pressed', 'true');
    await user.click(screen.getByRole('button', { name: '7 days' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/trends?range=7d', expect.anything()));
    expect(screen.getByRole('button', { name: '7 days' })).toHaveAttribute('aria-pressed', 'true');
    expect(await screen.findByRole('img', { name: 'Overnight HRV over 7 days' })).toBeInTheDocument();
  });

  it('can be driven from the keyboard: Tab to the range control, Enter selects', async () => {
    const fetchMock = mockApi({ '/api/trends?range=4w': trendsResponse('4w'), '/api/trends?range=3m': trendsResponse('3m') });
    const user = userEvent.setup();
    renderAt('/health');
    await screen.findByText('Avg 60 ms · range 55 to 70');
    screen.getByRole('button', { name: '3 months' }).focus();
    await user.keyboard('{Enter}');
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/trends?range=3m', expect.anything()));
  });

  it('shows "unavailable" for every metric the watch has no readings for', async () => {
    mockApi({ '/api/trends?range=4w': trendsResponse('4w', true) });
    renderAt('/health');
    expect(await screen.findByText('No overnight HRV readings in this range.')).toBeInTheDocument();
    expect(screen.getAllByText('unavailable').length).toBeGreaterThanOrEqual(8);
    expect(screen.queryByRole('img', { name: /Overnight HRV/ })).not.toBeInTheDocument();
  });

  it('shows an error state, and the links stay available', async () => {
    mockApi({ '/api/trends?range=4w': () => new Response(JSON.stringify({ error: 'down' }), { status: 500, headers: { 'content-type': 'application/json' } }) });
    renderAt('/health');
    expect(await screen.findByRole('alert', {}, { timeout: 3000 })).toHaveTextContent('Could not load health trends');
    expect(screen.getByRole('link', { name: /^Sleep/ })).toBeInTheDocument();
  });
});
