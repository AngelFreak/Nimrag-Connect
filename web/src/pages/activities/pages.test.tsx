import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ActivityDetail } from '../../api/generated';
import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import type { StrengthResponse } from '../../api/generated';
import type { StrengthSet } from '../../api/generated/StrengthSet';
import { ActivitiesPage } from '../ActivitiesPage';
import { ActivityDetailPage } from '../ActivityDetailPage';
import { StrengthPage } from '../StrengthPage';

// uPlot needs a canvas and MapLibre needs WebGL; neither exists in jsdom. The stubs keep the
// accessible name and the text summary, which is what the pages promise.
vi.mock('../../components/SeriesChart', () => ({
  SeriesChart: ({ label, summary }: { label: string; summary: string }) => (
    <div role="img" aria-label={label}>
      <p>{summary}</p>
    </div>
  ),
}));
vi.mock('../../components/MapView', () => ({
  MapView: ({ label, track }: { label?: string; track?: [number, number][] }) => <div role="img" aria-label={label} data-first={JSON.stringify(track?.[0])} />,
}));

const ID_RUN = 'a'.repeat(64);
const ID_STR = 'b'.repeat(64);
const ID_PREV = 'c'.repeat(64);

function listItem(over: Partial<ActivityListItem> = {}): ActivityListItem {
  return {
    id: ID_RUN,
    kind: 'run',
    sport_name: 'running',
    sub_sport_name: 'generic',
    start: '2026-11-16T17:40:00+00:00',
    date: '2026-11-16',
    duration_s: 2280,
    distance_m: 6400,
    avg_pace_s_per_km: 356,
    avg_hr_bpm: 151,
    max_hr_bpm: 170,
    calories_kcal: 468,
    ascent_m: 42,
    sets: null,
    volume_kg: null,
    exercises: null,
    ...over,
  };
}

function detail(over: Partial<ActivityDetail> = {}): ActivityDetail {
  return {
    ...listItem(),
    sample: true,
    end: null,
    elapsed_s: 2300,
    avg_speed_mps: 2.8,
    max_speed_mps: 4,
    descent_m: 40,
    avg_cadence: 86,
    aerobic_te: 3.2,
    anaerobic_te: null,
    training_load: 76,
    vo2max_derived: 46.2,
    has_track: true,
    has_strength: false,
    hr_below_zone1_s: null,
    hr_zones: [
      { zone: 3, duration_s: 1080 },
      { zone: 4, duration_s: 480 },
    ],
    laps: [{ index: 1, start: null, distance_m: 1000, duration_s: 365, avg_pace_s_per_km: 365, avg_hr_bpm: 140, max_hr_bpm: 150, ascent_m: 5 }],
    series: { elapsed_s: [0, 60, 120], heart_rate_bpm: [100, 140, 150], speed_mps: [2.5, 3, 2.8], altitude_m: [10, 12, 11], distance_m: [0, 100, 200], cadence: [null, 80, 82] },
    ...over,
  };
}

function sset(idx: number, over: Partial<StrengthSet> = {}): StrengthSet {
  return {
    idx,
    reps: 10,
    weight_kg: 22,
    duration_s: null,
    exercise: 'Bench press',
    edited: false,
    original_reps: 10,
    original_weight_kg: 22,
    original_exercise: 'Bench press',
    ...over,
  };
}

function strength(over: Partial<StrengthResponse> = {}): StrengthResponse {
  return {
    sample: false,
    id: ID_STR,
    start: '2026-11-18T18:05:00+00:00',
    duration_s: 2520,
    avg_hr_bpm: 112,
    max_hr_bpm: 148,
    sets: 3,
    volume_kg: 660,
    exercises: [
      { name: 'Bench press', sets: [sset(0), sset(1)], volume_kg: 440 },
      { name: 'Plank', sets: [sset(2, { reps: null, weight_kg: null, duration_s: 60, exercise: 'Plank', original_reps: null, original_weight_kg: null, original_exercise: 'Plank' })], volume_kg: null },
    ],
    heart_rate: [
      { time: '2026-11-18T18:05:00+00:00', value: 90 },
      { time: '2026-11-18T18:06:00+00:00', value: 120 },
    ],
    previous: { id: ID_PREV, start: '2026-11-15T10:20:00+00:00', duration_s: 2700, sets: 3, volume_kg: 600 },
    ...over,
  };
}

type Handler = (url: URL, init: RequestInit | undefined) => { status?: number; body: unknown } | Promise<{ status?: number; body: unknown }>;
let handler: Handler;
const calls: { url: URL; init: RequestInit | undefined }[] = [];

beforeEach(() => {
  calls.length = 0;
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = new URL(String(input), 'http://localhost');
      calls.push({ url, init });
      const r = await handler(url, init);
      return new Response(JSON.stringify(r.body), { status: r.status ?? 200, headers: { 'content-type': 'application/json' } });
    }),
  );
});
afterEach(() => vi.unstubAllGlobals());

function renderAt(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/activities" element={<ActivitiesPage />} />
          <Route path="/strength" element={<ActivitiesPage strength />} />
          <Route path="/activities/:id" element={<ActivityDetailPage />} />
          <Route path="/activities/:id/strength" element={<StrengthPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('ActivitiesPage', () => {
  it('lists activities from the API, links strength rows to the strength view and loads more with next_before', async () => {
    const user = userEvent.setup();
    handler = (url) => {
      if (url.searchParams.get('before')) return { body: { sample: false, items: [listItem({ id: 'd'.repeat(64), date: '2026-10-02', start: '2026-10-02T12:00:00+00:00', kind: 'walk', distance_m: 4800 })], next_before: null } };
      return {
        body: {
          sample: true,
          items: [listItem(), listItem({ id: ID_STR, kind: 'strength', distance_m: null, avg_pace_s_per_km: null, sets: 18, volume_kg: 3300, duration_s: 2520, date: '2026-11-18', start: '2026-11-18T18:05:00+00:00' })],
          next_before: '2026-11-16T17:40:00+00:00',
        },
      };
    };
    renderAt('/activities');
    expect(await screen.findByText('6.40 km')).toBeInTheDocument();
    expect(screen.getByText('Sample data')).toBeInTheDocument();
    expect(screen.getByText('18 sets · 3,300 kg')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Strength/ })).toHaveAttribute('href', `/activities/${ID_STR}/strength`);
    expect(screen.getByRole('link', { name: /Run/ })).toHaveAttribute('href', `/activities/${ID_RUN}`);

    await user.click(screen.getByRole('button', { name: 'Load more' }));
    expect(await screen.findByText('4.80 km')).toBeInTheDocument();
    const last = calls[calls.length - 1]!.url;
    expect(last.searchParams.get('before')).toBe('2026-11-16T17:40:00+00:00');
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('filters by type through the API and shows an empty state', async () => {
    const user = userEvent.setup();
    handler = (url) => ({ body: { sample: false, items: url.searchParams.get('type') === 'swim' ? [] : [listItem()], next_before: null } });
    renderAt('/activities');
    await screen.findByText('6.40 km');
    expect(screen.queryByText('Sample data')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Swim' }));
    expect(await screen.findByText('No activities of this type')).toBeInTheDocument();
    expect(calls.some((c) => c.url.searchParams.get('type') === 'swim')).toBe(true);
    expect(screen.getByRole('button', { name: 'Swim' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('/strength is the strength list: filtered through the API, titled Strength, and keeps its path', async () => {
    const user = userEvent.setup();
    handler = () => ({ body: { sample: false, items: [listItem({ id: ID_STR, kind: 'strength', distance_m: null, avg_pace_s_per_km: null, sets: 18, volume_kg: 3300 })], next_before: null } });
    renderAt('/strength');
    expect(await screen.findByText('18 sets · 3,300 kg')).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 1, name: 'Strength' })).toBeInTheDocument();
    expect(calls.some((c) => c.url.searchParams.get('type') === 'strength')).toBe(true);
    expect(screen.getByRole('button', { name: 'Strength' })).toHaveAttribute('aria-pressed', 'true');
    // Another type leaves the Strength path for the Activities filter.
    handler = () => ({ body: { sample: false, items: [listItem()], next_before: null } });
    await user.click(screen.getByRole('button', { name: 'Run' }));
    expect(await screen.findByText('6.40 km')).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 1, name: 'Activities' })).toBeInTheDocument();
  });

  it('the Strength chip on /activities goes to /strength', async () => {
    const user = userEvent.setup();
    handler = () => ({ body: { sample: false, items: [listItem()], next_before: null } });
    renderAt('/activities');
    await screen.findByText('6.40 km');
    await user.click(screen.getByRole('button', { name: 'Strength' }));
    expect(await screen.findByRole('heading', { level: 1, name: 'Strength' })).toBeInTheDocument();
  });

  it('shows "unavailable" for missing values, never 0', async () => {
    handler = () => ({ body: { sample: false, items: [listItem({ duration_s: null, distance_m: null, avg_pace_s_per_km: null, avg_hr_bpm: null, calories_kcal: null, kind: 'other', sport_name: 'generic' })], next_before: null } });
    renderAt('/activities');
    expect(await screen.findByText('unavailable')).toBeInTheDocument();
    expect(screen.queryByText(/^0/)).not.toBeInTheDocument();
  });

  it('shows an error with a retry', async () => {
    const user = userEvent.setup();
    let fail = true;
    handler = () => (fail ? { status: 500, body: { error: 'database is locked' } } : { body: { sample: false, items: [listItem()], next_before: null } });
    renderAt('/activities');
    expect(await screen.findByRole('alert')).toHaveTextContent('database is locked');
    fail = false;
    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByText('6.40 km')).toBeInTheDocument();
  });

  it('shows a loading state before the data arrives', () => {
    handler = () => new Promise(() => {});
    renderAt('/activities');
    expect(screen.getByText('Loading activities')).toBeInTheDocument();
  });
});

describe('ActivityDetailPage', () => {
  it('renders stats, charts with text summaries, zones, laps and labels VO2 max as derived', async () => {
    handler = (url) => {
      if (url.pathname.endsWith('/track')) return { body: { sample: true, id: ID_RUN, available: true, points: [[56.1, 10.2], [56.2, 10.3]], start: null, end: null, bounds: null } };
      return { body: detail() };
    };
    renderAt(`/activities/${ID_RUN}`);
    expect(await screen.findByText('6.40 km')).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 1, name: 'Run' })).toBeInTheDocument();
    expect(screen.getByText('5:56 /km')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Heart rate over the activity' })).toBeInTheDocument();
    expect(screen.getByText(/Heart rate over 2 minutes: lowest 100, average 130, highest 150/)).toBeInTheDocument();
    expect(screen.getByText(/Pace over 2 minutes/)).toBeInTheDocument();
    expect(screen.getByText(/Elevation over 2 minutes: lowest 10 m, highest 12 m/)).toBeInTheDocument();
    expect(screen.getByText('VO2 max (derived)')).toBeInTheDocument();
    expect(screen.getByText('46.2 ml/kg/min')).toBeInTheDocument();
    expect(screen.getByText(/not Garmin's published value/)).toBeInTheDocument();
    const zones = screen.getByRole('list', { name: 'Time in heart rate zones' });
    expect(within(zones).getByText('18 min')).toBeInTheDocument();
    expect(within(zones).getAllByText('unavailable')).toHaveLength(3);
    expect(screen.getByRole('table', { name: 'Laps' })).toBeInTheDocument();
    // The API sends [lat, lon]; the map gets [lon, lat].
    const map = await screen.findByRole('img', { name: /^Route map: the 6\.40 km route of this activity/ });
    expect(map).toHaveAttribute('data-first', '[10.2,56.1]');
  });

  it('has no placeholder caption or "Minutes since" note', async () => {
    handler = (url) => {
      if (url.pathname.endsWith('/track')) return { body: { sample: true, id: ID_RUN, available: true, points: [[56.1, 10.2], [56.2, 10.3]], start: null, end: null, bounds: null } };
      return { body: detail({ series: { elapsed_s: [0, 600, 1200, 1800], heart_rate_bpm: [100, 140, 150, 120], speed_mps: [2.5, 3, 2.8, 2.9], altitude_m: [10, 12, 11, 10], distance_m: [0, 100, 200, 300], cadence: [null, 80, 82, 81] } }) };
    };
    renderAt(`/activities/${ID_RUN}`);
    await screen.findByText('6.40 km');
    expect(screen.queryByText(/Minutes since the start/)).not.toBeInTheDocument();
    expect(screen.queryByText(/placeholder/i)).not.toBeInTheDocument();
  });

  it('leaves out tiles that have no value instead of saying "unavailable"', async () => {
    handler = () => ({ body: detail({ avg_cadence: null, descent_m: null, aerobic_te: null, anaerobic_te: null, training_load: null, vo2max_derived: null, avg_hr_bpm: null, ascent_m: null, calories_kcal: null }) });
    renderAt(`/activities/${ID_RUN}`);
    await screen.findByText('6.40 km');
    for (const label of ['Cadence', 'Descent', 'Aerobic effect', 'Anaerobic effect', 'Training load', 'VO2 max (derived)', 'Avg HR', 'Climb', 'Energy']) {
      expect(screen.queryByText(label)).not.toBeInTheDocument();
    }
    // What is there stays: distance, time, pace, max heart rate.
    expect(screen.getAllByText('Distance').length).toBeGreaterThan(0);
    expect(screen.getByText('Max HR')).toBeInTheDocument();
    // The only "unavailable" left are the heart rate zones the file does not list.
    expect(screen.getAllByText('unavailable')).toHaveLength(3);
  });

  it('shows no tile grid and no tiles for an activity with no stats at all', async () => {
    handler = () => ({ body: detail({ has_track: false, series: null, vo2max_derived: null, aerobic_te: null, training_load: null, avg_cadence: null, descent_m: null, avg_hr_bpm: null, max_hr_bpm: null, ascent_m: null, calories_kcal: null, hr_zones: [], laps: [] }) });
    renderAt(`/activities/${ID_RUN}`);
    expect(await screen.findByText('This activity has no GPS track.')).toBeInTheDocument();
    expect(screen.queryByText('Aerobic effect')).not.toBeInTheDocument();
    expect(screen.queryByText('Cadence')).not.toBeInTheDocument();
  });

  it('shows unavailable for missing metrics and no map for an activity without a track', async () => {
    handler = () => ({
      body: detail({ has_track: false, series: null, vo2max_derived: null, aerobic_te: null, training_load: null, avg_hr_bpm: null, hr_zones: [], laps: [], ascent_m: null }),
    });
    renderAt(`/activities/${ID_RUN}`);
    expect(await screen.findByText('This activity has no GPS track.')).toBeInTheDocument();
    expect(screen.getByText('No heart rate samples were recorded.')).toBeInTheDocument();
    expect(screen.queryByText(/ml\/kg\/min/)).not.toBeInTheDocument();
    expect(screen.getAllByText('unavailable').length).toBeGreaterThan(5);
    expect(screen.queryByText(/^0 bpm/)).not.toBeInTheDocument();
  });

  it('links a strength activity to its strength view', async () => {
    handler = () => ({ body: detail({ id: ID_STR, kind: 'strength', has_strength: true, has_track: false }) });
    renderAt(`/activities/${ID_STR}`);
    expect(await screen.findByRole('link', { name: 'View strength sets' })).toHaveAttribute('href', `/activities/${ID_STR}/strength`);
  });

  it('says so when the activity does not exist', async () => {
    handler = () => ({ status: 404, body: { error: 'no such activity' } });
    renderAt(`/activities/${ID_RUN}`);
    expect(await screen.findByText('Activity not found')).toBeInTheDocument();
  });
});

describe('StrengthPage', () => {
  it('shows sets grouped by exercise, bodyweight and time-based sets', async () => {
    handler = () => ({ body: strength() });
    renderAt(`/activities/${ID_STR}/strength`);
    expect(await screen.findByText('Bench press')).toBeInTheDocument();
    expect(screen.getByText(/2 sets/)).toHaveTextContent('2 sets · 10, 10 reps');
    expect(screen.getByText('22 kg')).toBeInTheDocument();
    expect(screen.getByText(/1 set/)).toHaveTextContent('1 set · 60 s each');
    expect(screen.getByText('Bodyweight')).toBeInTheDocument();
    expect(screen.getByText('42:00')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Heart rate during the session' })).toBeInTheDocument();
  });

  it('edits a set, validates, and sends only the changed fields', async () => {
    const user = userEvent.setup();
    let saved: unknown;
    handler = (_url, init) => {
      if (init?.method === 'PATCH') {
        saved = JSON.parse(String(init.body));
        return { body: strength({ exercises: [{ name: 'Bench press', sets: [sset(0, { reps: 12, edited: true }), sset(1)], volume_kg: 0 }] }) };
      }
      return { body: strength() };
    };
    renderAt(`/activities/${ID_STR}/strength`);
    await user.click(await screen.findByRole('button', { name: 'Edit sets' }));

    const reps = screen.getByRole('textbox', { name: 'Bench press, set 1, reps' });
    await user.clear(reps);
    await user.type(reps, 'abc');
    expect(await screen.findByText(/Reps: a whole number/)).toBeInTheDocument();
    expect(reps).toHaveAttribute('aria-invalid', 'true');
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(screen.getByRole('alert')).toHaveTextContent('Fix 1 set before saving');
    expect(calls.some((c) => c.init?.method === 'PATCH')).toBe(false);

    await user.clear(reps);
    await user.type(reps, '12');
    expect(screen.queryByText(/Reps: a whole number/)).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(saved).toEqual({ sets: [{ idx: 0, reps: 12 }] }));
    const patch = calls.find((c) => c.init?.method === 'PATCH')!;
    expect((patch.init?.headers as Record<string, string>)['X-Nimrag']).toBe('1');
    expect(await screen.findByRole('status')).toHaveTextContent('Saved. 1 set changed.');
    expect(screen.getByText('Edited')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Edit sets' })).toBeInTheDocument();
  });

  it('keeps the edits and says why when saving fails', async () => {
    const user = userEvent.setup();
    handler = (_url, init) => (init?.method === 'PATCH' ? { status: 400, body: { error: 'reps must be a whole number 0 to 10000' } } : { body: strength() });
    renderAt(`/activities/${ID_STR}/strength`);
    await user.click(await screen.findByRole('button', { name: 'Edit sets' }));
    const w = screen.getByRole('textbox', { name: 'Bench press, set 2, weight in kilograms' });
    await user.clear(w);
    await user.type(w, '30,5');
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Not saved. reps must be a whole number 0 to 10000 Your changes are still here.');
    expect(screen.getByRole('textbox', { name: 'Bench press, set 2, weight in kilograms' })).toHaveValue('30,5');
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeEnabled();
    const body = JSON.parse(String(calls.find((c) => c.init?.method === 'PATCH')!.init!.body));
    expect(body).toEqual({ sets: [{ idx: 1, weight_kg: 30.5 }] });
  });

  it('resets an edited set to the watch value with reset: true', async () => {
    const user = userEvent.setup();
    let saved: unknown;
    const edited = strength({ exercises: [{ name: 'Bench press', sets: [sset(0, { reps: 12, edited: true })], volume_kg: 264 }] });
    handler = (_url, init) => {
      if (init?.method === 'PATCH') {
        saved = JSON.parse(String(init.body));
        return { body: strength() };
      }
      return { body: edited };
    };
    renderAt(`/activities/${ID_STR}/strength`);
    await user.click(await screen.findByRole('button', { name: 'Edit sets' }));
    await user.click(screen.getByRole('button', { name: /Reset set 1 of Bench press/ }));
    expect(screen.getByRole('textbox', { name: 'Bench press, set 1, reps' })).toHaveValue('10');
    await user.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(saved).toEqual({ sets: [{ idx: 0, reset: true }] }));
  });

  it('discarding edits sends nothing', async () => {
    const user = userEvent.setup();
    handler = () => ({ body: strength() });
    renderAt(`/activities/${ID_STR}/strength`);
    await user.click(await screen.findByRole('button', { name: 'Edit sets' }));
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();
    await user.type(screen.getByRole('textbox', { name: 'Bench press, set 1, reps' }), '5');
    await user.click(screen.getByRole('button', { name: 'Discard changes' }));
    expect(screen.queryByRole('textbox', { name: 'Bench press, set 1, reps' })).not.toBeInTheDocument();
    expect(calls.some((c) => c.init?.method === 'PATCH')).toBe(false);
  });

  it('compares with the previous session exercise by exercise and lists what is only in the last one', async () => {
    const user = userEvent.setup();
    handler = (url) => {
      if (url.pathname === `/api/activities/${ID_PREV}/strength`) {
        return {
          body: strength({
            id: ID_PREV,
            duration_s: 2700,
            exercises: [
              { name: 'Bench press', sets: [sset(0, { weight_kg: 20 }), sset(1, { weight_kg: 20 })], volume_kg: 400 },
              { name: 'Squat', sets: [sset(2, { exercise: 'Squat' })], volume_kg: 220 },
            ],
          }),
        };
      }
      return { body: strength() };
    };
    renderAt(`/activities/${ID_STR}/strength`);
    await user.click(await screen.findByRole('button', { name: 'Compare to last' }));
    expect(await screen.findByText('Compared with Sun 15 Nov')).toBeInTheDocument();
    const table = await screen.findByRole('table', { name: /this session against the last/i });
    expect(within(table).getByText('was 20 kg (+2 kg)')).toBeInTheDocument();
    expect(screen.getByText(/Only in the last session: Squat/)).toBeInTheDocument();
    expect(screen.getByText('Last: 45:00 (-3 min)')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Hide comparison' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('says so when there is no earlier session', async () => {
    const user = userEvent.setup();
    handler = () => ({ body: strength({ previous: null }) });
    renderAt(`/activities/${ID_STR}/strength`);
    await user.click(await screen.findByRole('button', { name: 'Compare to last' }));
    expect(await screen.findByText('No earlier strength session')).toBeInTheDocument();
  });

  it('shows unavailable when the session has no volume or heart rate', async () => {
    handler = () => ({ body: strength({ volume_kg: null, duration_s: null, avg_hr_bpm: null, max_hr_bpm: null, heart_rate: [] }) });
    renderAt(`/activities/${ID_STR}/strength`);
    await screen.findByText('Bench press');
    expect(screen.getByText('No heart rate samples were recorded.')).toBeInTheDocument();
    expect(screen.getAllByText('unavailable').length).toBeGreaterThanOrEqual(2);
  });

  it('shows an error state with retry', async () => {
    handler = () => ({ status: 500, body: { error: 'boom' } });
    renderAt(`/activities/${ID_STR}/strength`);
    expect(await screen.findByRole('alert')).toHaveTextContent('boom');
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
  });
});

// ---- the wide-screen layout (>= 1024 px): cards, toolbar, two-column activity page ----------------

describe('on a wide screen', () => {
  const real = window.matchMedia;
  beforeEach(() => {
    window.matchMedia = ((query: string) => ({ matches: query.includes('min-width: 1024px'), media: query, addEventListener: () => {}, removeEventListener: () => {} })) as unknown as typeof window.matchMedia;
  });
  afterEach(() => {
    window.matchMedia = real;
  });

  const track = { sample: true, id: ID_RUN, available: true, points: [[56.1, 10.2], [56.2, 10.3]], start: null, end: null, bounds: null };
  const runAndStrength = () => ({
    sample: false,
    items: [
      listItem(),
      listItem({ id: ID_STR, kind: 'strength', distance_m: null, avg_pace_s_per_km: null, sets: 18, volume_kg: 3300, exercises: 6, duration_s: 2520, date: '2026-11-18', start: '2026-11-18T18:05:00+00:00' }),
    ],
    next_before: null,
  });

  describe('ActivitiesPage', () => {
    it('lists the activities as cards: a real link on the name, five labelled figures, and the title-row actions', async () => {
      handler = () => ({ body: runAndStrength() });
      renderAt('/activities');
      const list = await screen.findByRole('list', { name: 'Activities' });
      const rows = within(list).getAllByRole('listitem');
      expect(rows).toHaveLength(2);
      expect(screen.getByRole('heading', { level: 1, name: 'Activities' })).toBeInTheDocument();
      // Newest first: the strength session (18 Nov), then the run (16 Nov).
      // Run: distance, time, pace, ascent, heart rate; each value sits with its label.
      const run = within(rows[1]!);
      expect(run.getByRole('link', { name: /^Run/ })).toHaveAttribute('href', `/activities/${ID_RUN}`);
      expect(run.getByText('Distance').closest('div')).toHaveTextContent('6.40 km');
      expect(run.getByText('Avg pace').closest('div')).toHaveTextContent('5:56 /km');
      expect(run.getByText('Total ascent').closest('div')).toHaveTextContent('42 m');
      expect(run.getByText('Avg HR').closest('div')).toHaveTextContent('151 bpm');
      expect(run.getByText('Nov 16')).toBeInTheDocument();
      // Strength: exercises, sets, volume instead; its link goes to the sets view.
      const str = within(rows[0]!);
      expect(str.getByRole('link', { name: /^Strength/ })).toHaveAttribute('href', `/activities/${ID_STR}/strength`);
      expect(str.getByText('Sets').closest('div')).toHaveTextContent('18');
      expect(str.getByText('Volume').closest('div')).toHaveTextContent('3,300 kg');
      expect(str.getByText('Exercises').closest('div')).toHaveTextContent('6');
      // Actions that exist in Nimrag.
      const actions = screen.getByRole('navigation', { name: 'Activity actions' });
      expect(within(actions).getByRole('link', { name: '+ Workout' })).toHaveAttribute('href', '/workouts/new');
      expect(within(actions).getByRole('link', { name: 'Import' })).toHaveAttribute('href', '/profile/import-garmin');
      expect(within(actions).getByRole('link', { name: 'Export data' })).toHaveAttribute('href', '/api/export');
    });

    it('the icon filter keeps the same values: it asks the API for the type and shows an empty state', async () => {
      const user = userEvent.setup();
      handler = (url) => ({ body: { sample: false, items: url.searchParams.get('type') === 'swim' ? [] : [listItem()], next_before: null } });
      renderAt('/activities');
      await screen.findByRole('list', { name: 'Activities' });
      const filter = screen.getByRole('group', { name: 'Filter by type' });
      expect(within(filter).getAllByRole('button').map((b) => b.getAttribute('aria-label'))).toEqual(['All', 'Run', 'Strength', 'Walk', 'Hike', 'Cycle', 'Swim', 'Training', 'Other']);
      expect(within(filter).getByRole('button', { name: 'All' })).toHaveAttribute('aria-pressed', 'true');
      await user.click(within(filter).getByRole('button', { name: 'Swim' }));
      expect(await screen.findByText('No activities of this type')).toBeInTheDocument();
      expect(calls.some((c) => c.url.searchParams.get('type') === 'swim')).toBe(true);
      expect(screen.getByRole('button', { name: 'Swim' })).toHaveAttribute('aria-pressed', 'true');
    });

    it('the Strength button goes to /strength, which is the strength list', async () => {
      const user = userEvent.setup();
      handler = () => ({ body: runAndStrength() });
      renderAt('/activities');
      await screen.findByRole('list', { name: 'Activities' });
      await user.click(screen.getByRole('button', { name: 'Strength' }));
      expect(await screen.findByRole('heading', { level: 1, name: 'Strength' })).toBeInTheDocument();
      expect(screen.getByRole('list', { name: 'Strength sessions' })).toBeInTheDocument();
      expect(calls.some((c) => c.url.searchParams.get('type') === 'strength')).toBe(true);
    });

    it('searches the loaded rows by type and says so when nothing matches', async () => {
      const user = userEvent.setup();
      handler = () => ({ body: runAndStrength() });
      renderAt('/activities');
      await screen.findByRole('list', { name: 'Activities' });
      await user.type(screen.getByRole('searchbox', { name: 'Search loaded activities' }), 'stren');
      const rows = within(screen.getByRole('list', { name: 'Activities' })).getAllByRole('listitem');
      expect(rows).toHaveLength(1);
      expect(within(rows[0]!).getByRole('link', { name: /^Strength/ })).toBeInTheDocument();
      await user.type(screen.getByRole('searchbox', { name: 'Search loaded activities' }), 'xyz');
      expect(await screen.findByText(/No loaded activity matches/)).toBeInTheDocument();
      expect(screen.queryByRole('list', { name: 'Activities' })).not.toBeInTheDocument();
    });

    it('sorts the loaded rows from the select (missing values last) and loads more with next_before', async () => {
      const user = userEvent.setup();
      handler = (url) => {
        if (url.searchParams.get('before')) return { body: { sample: false, items: [listItem({ id: 'd'.repeat(64), date: '2026-10-02', start: '2026-10-02T12:00:00+00:00', kind: 'walk', distance_m: 4800, duration_s: 4000 })], next_before: null } };
        return { body: { sample: false, items: [listItem({ duration_s: 3000 }), listItem({ id: ID_STR, kind: 'strength', duration_s: 600, date: '2026-11-18', start: '2026-11-18T18:05:00+00:00' })], next_before: '2026-11-16T17:40:00+00:00' } };
      };
      renderAt('/activities');
      await screen.findByRole('list', { name: 'Activities' });
      const order = () => within(screen.getByRole('list', { name: 'Activities' })).getAllByRole('link').map((l) => l.textContent?.split(',')[0]);
      expect(order()).toEqual(['Strength', 'Run']);
      await user.selectOptions(screen.getByRole('combobox', { name: 'Sort loaded activities' }), 'longest');
      expect(order()).toEqual(['Run', 'Strength']);
      expect(screen.getByText(/Search and sorting apply to the 2 activities loaded so far/)).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Load more' }));
      await waitFor(() => expect(order()).toHaveLength(3));
      expect(order()[0]).toBe('Walk');
      expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
    });

    it('says "unavailable" for a figure the watch did not record, never 0', async () => {
      handler = () => ({ body: { sample: false, items: [listItem({ kind: 'other', sport_name: 'generic', duration_s: null, distance_m: null, avg_hr_bpm: null, max_hr_bpm: null, calories_kcal: null })], next_before: null } });
      renderAt('/activities');
      await screen.findByRole('list', { name: 'Activities' });
      expect(screen.getAllByText('unavailable')).toHaveLength(5);
      expect(screen.queryByText(/^0/)).not.toBeInTheDocument();
    });
  });

  describe('ActivityDetailPage', () => {
    const fullDetail = () => {
      handler = (url) => (url.pathname.endsWith('/track') ? { body: track } : { body: detail() });
    };

    it('has a breadcrumb, a title, five headline figures, the map and a chart for each series', async () => {
      fullDetail();
      renderAt(`/activities/${ID_RUN}`);
      expect(await screen.findByRole('heading', { level: 1, name: 'Run' })).toBeInTheDocument();
      const crumb = screen.getByRole('navigation', { name: 'Breadcrumb' });
      expect(within(crumb).getByRole('link', { name: 'Activities' })).toHaveAttribute('href', '/activities');
      expect(crumb).toHaveTextContent('Running · on November 16, 2026 @ 17:40');
      for (const [label, value] of [
        ['Distance', '6.40 km'],
        ['Time', '38:00'],
        ['Avg pace', '5:56 /km'],
        ['Total ascent', '42 m'],
        ['Avg HR', '151 bpm'],
      ] as const) {
        expect(screen.getAllByText(label)[0]!.closest('div')).toHaveTextContent(value);
      }
      const map = await screen.findByRole('img', { name: /^Route map: the 6\.40 km route of this activity/ });
      // The API sends [lat, lon]; the map gets [lon, lat].
      expect(map).toHaveAttribute('data-first', '[10.2,56.1]');
      for (const name of ['Heart rate over the activity', 'Pace over the activity', 'Elevation over the activity', 'Cadence over the activity']) {
        expect(screen.getByRole('img', { name })).toBeInTheDocument();
      }
      // One chart title and one Stats group are called Pace.
      expect(screen.getAllByRole('heading', { level: 3, name: 'Pace' })).toHaveLength(2);
      expect(screen.getByText(/Pace over 2 minutes/)).toBeInTheDocument();
    });

    it('offers a Time / Distance switch when the samples have a distance, and not otherwise', async () => {
      const user = userEvent.setup();
      fullDetail();
      renderAt(`/activities/${ID_RUN}`);
      const sw = await screen.findByRole('group', { name: 'Chart x axis' });
      expect(within(sw).getByRole('button', { name: 'Time' })).toHaveAttribute('aria-pressed', 'true');
      await user.click(within(sw).getByRole('button', { name: 'Distance' }));
      expect(within(sw).getByRole('button', { name: 'Distance' })).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByRole('img', { name: 'Heart rate over the activity' })).toBeInTheDocument();
    });

    it('has no switch when the file has no distance samples', async () => {
      handler = (url) =>
        url.pathname.endsWith('/track')
          ? { body: track }
          : { body: detail({ series: { elapsed_s: [0, 60, 120], heart_rate_bpm: [100, 140, 150], speed_mps: [2.5, 3, 2.8], altitude_m: [10, 12, 11], distance_m: [null, null, null], cadence: [null, null, null] } }) };
      renderAt(`/activities/${ID_RUN}`);
      await screen.findByRole('heading', { level: 1, name: 'Run' });
      expect(screen.queryByRole('group', { name: 'Chart x axis' })).not.toBeInTheDocument();
      expect(screen.queryByRole('img', { name: 'Cadence over the activity' })).not.toBeInTheDocument();
    });

    it('tabs: Stats first as groups, then Laps and Time in Zones, with the arrow keys moving between them', async () => {
      const user = userEvent.setup();
      fullDetail();
      renderAt(`/activities/${ID_RUN}`);
      const tabs = await screen.findByRole('tablist', { name: 'Activity details' });
      expect(within(tabs).getAllByRole('tab').map((t) => t.textContent)).toEqual(['Stats', 'Laps', 'Time in Zones']);
      const panel = screen.getByRole('tabpanel');
      for (const group of ['Distance', 'Timing', 'Pace', 'Heart rate', 'Elevation', 'Cadence', 'Energy']) {
        expect(within(panel).getByRole('heading', { level: 3, name: group })).toBeInTheDocument();
      }
      expect(within(panel).getByText('Total descent').closest('div')).toHaveTextContent('40 m');
      expect(within(panel).getByText('Min elev').closest('div')).toHaveTextContent('10 m');

      await user.click(within(tabs).getByRole('tab', { name: 'Laps' }));
      expect(screen.getByRole('table', { name: 'Laps' })).toBeInTheDocument();

      within(tabs).getByRole('tab', { name: 'Laps' }).focus();
      await user.keyboard('{ArrowRight}');
      expect(within(tabs).getByRole('tab', { name: 'Time in Zones' })).toHaveAttribute('aria-selected', 'true');
      const zones = screen.getByRole('list', { name: 'Time in heart rate zones' });
      expect(within(zones).getByText('18 min')).toBeInTheDocument();
      expect(within(zones).getAllByText('unavailable')).toHaveLength(3);
      await user.keyboard('{Home}');
      expect(within(tabs).getByRole('tab', { name: 'Stats' })).toHaveAttribute('aria-selected', 'true');
    });

    it('the side column has the training effect and labels VO2 max as derived', async () => {
      fullDetail();
      renderAt(`/activities/${ID_RUN}`);
      const side = await screen.findByRole('complementary', { name: 'More about this activity' });
      expect(within(side).getByText('Aerobic effect').closest('div')).toHaveTextContent('3.2');
      expect(within(side).queryByText('Anaerobic effect')).not.toBeInTheDocument();
      expect(within(side).getByText('Training load').closest('div')).toHaveTextContent('76');
      expect(within(side).getByText('VO2 max (derived)').closest('div')).toHaveTextContent('46.2 ml/kg/min');
      expect(within(side).getByText(/not Garmin's published value/)).toBeInTheDocument();
    });

    it('leaves out the figures that have no value instead of saying "unavailable"', async () => {
      handler = () => ({ body: detail({ avg_cadence: null, descent_m: null, aerobic_te: null, anaerobic_te: null, training_load: null, vo2max_derived: null, avg_hr_bpm: null, ascent_m: null, calories_kcal: null, series: null, has_track: false }) });
      renderAt(`/activities/${ID_RUN}`);
      await screen.findByRole('heading', { level: 1, name: 'Run' });
      expect(screen.getByText('This activity has no GPS track.')).toBeInTheDocument();
      for (const label of ['Total ascent', 'Avg HR', 'Aerobic effect', 'Training load', 'VO2 max (derived)', 'Avg cadence', 'Total descent', 'Calories']) {
        expect(screen.queryByText(label)).not.toBeInTheDocument();
      }
      // Distance, time and pace are there; the only "unavailable" left are the zones the file does not list (the tab is not open).
      expect(screen.queryByText('unavailable')).not.toBeInTheDocument();
      expect(screen.getByText('No heart rate samples were recorded.')).toBeInTheDocument();
    });

    it('a strength activity links to its sets in the side column', async () => {
      handler = () => ({ body: detail({ id: ID_STR, kind: 'strength', has_strength: true, has_track: false, distance_m: null, sets: 18, volume_kg: 3300 }) });
      renderAt(`/activities/${ID_STR}`);
      expect(await screen.findByRole('link', { name: 'View strength sets' })).toHaveAttribute('href', `/activities/${ID_STR}/strength`);
      expect(screen.getAllByText('Volume')[0]!.closest('div')).toHaveTextContent('3,300 kg');
    });

    it('says so when the activity does not exist', async () => {
      handler = () => ({ status: 404, body: { error: 'no such activity' } });
      renderAt(`/activities/${ID_RUN}`);
      expect(await screen.findByText('Activity not found')).toBeInTheDocument();
    });
  });

  describe('StrengthPage', () => {
    it('has the breadcrumb, headline figures, a Sets tab with the exercises and a link to the previous session', async () => {
      handler = () => ({ body: strength() });
      renderAt(`/activities/${ID_STR}/strength`);
      expect(await screen.findByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent('Strength training · on November 18, 2026 @ 18:05');
      expect(screen.getAllByText('Sets', { selector: 'dt' })[0]!.closest('div')).toHaveTextContent('3');
      expect(screen.getAllByText('Volume', { selector: 'dt' })[0]!.closest('div')).toHaveTextContent('660 kg');
      expect(screen.getByRole('img', { name: 'Heart rate during the session' })).toBeInTheDocument();
      expect(screen.getByRole('tab', { name: 'Sets' })).toHaveAttribute('aria-selected', 'true');
      expect(screen.getByText('Bench press')).toBeInTheDocument();
      expect(screen.getByText('Bodyweight')).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Open previous session' })).toHaveAttribute('href', `/activities/${ID_PREV}/strength`);
      expect(screen.getByRole('link', { name: /^Previous session/ })).toHaveAttribute('href', `/activities/${ID_PREV}/strength`);
    });

    it('compares with the last session in its own tab', async () => {
      const user = userEvent.setup();
      handler = (url) => {
        if (url.pathname === `/api/activities/${ID_PREV}/strength`) return { body: strength({ id: ID_PREV, previous: null }) };
        return { body: strength() };
      };
      renderAt(`/activities/${ID_STR}/strength`);
      await user.click(await screen.findByRole('tab', { name: 'Compare to last' }));
      expect(await screen.findByText(/Compared with/)).toBeInTheDocument();
      expect(await screen.findByRole('table', { name: 'Exercises, this session against the last' })).toBeInTheDocument();
    });

    it('edits a set and sends only what changed; the compare tab waits while editing', async () => {
      const user = userEvent.setup();
      let saved: unknown;
      handler = (_url, init) => {
        if (init?.method === 'PATCH') {
          saved = JSON.parse(String(init.body));
          return { body: strength() };
        }
        return { body: strength() };
      };
      renderAt(`/activities/${ID_STR}/strength`);
      await user.click(await screen.findByRole('button', { name: 'Edit sets' }));
      expect(screen.getByRole('tab', { name: 'Compare to last' })).toBeDisabled();
      const reps = screen.getByRole('textbox', { name: 'Bench press, set 1, reps' });
      await user.clear(reps);
      await user.type(reps, '12');
      await user.click(screen.getByRole('button', { name: 'Save changes' }));
      await waitFor(() => expect(saved).toBeDefined());
      expect(JSON.stringify(saved)).toContain('12');
      expect(await screen.findByText(/Saved\. 1 set changed\./)).toBeInTheDocument();
    });
  });
});
