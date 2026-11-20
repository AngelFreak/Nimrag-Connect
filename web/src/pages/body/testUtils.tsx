import { render } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { vi } from 'vitest';
import type { BodyResponse, CalendarResponse, MovementResponse, TrainingResponse } from '../../api/generated';

export function json(body: unknown, init: ResponseInit = {}): Response {
  return new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' }, ...init });
}

type Handler = (url: string, init: RequestInit | undefined) => Response | Promise<Response>;

/** Replaces fetch; the first handler whose path prefix matches answers, anything else is a 404. */
export function mockApi(routes: Record<string, Handler | unknown>) {
  const calls: { url: string; init: RequestInit | undefined }[] = [];
  const fn = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    calls.push({ url, init });
    const method = init?.method ?? 'GET';
    const key = Object.keys(routes).find((k) => {
      const [m, p] = k.includes(' ') ? k.split(' ') : ['GET', k];
      return m === method && url.startsWith(p!);
    });
    if (!key) return json({ error: `no mock for ${method} ${url}` }, { status: 404 });
    const h = routes[key];
    return typeof h === 'function' ? await (h as Handler)(url, init) : json(h);
  });
  vi.stubGlobal('fetch', fn);
  return { fn, calls };
}

function Where() {
  const l = useLocation();
  return <div data-testid="where">{l.pathname + l.search}</div>;
}

export function renderAt(ui: ReactNode, path: string, route = '*') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path={route} element={ui} />
        </Routes>
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

export const TODAY = { date: '2026-11-19' };

export function movement(over: Partial<MovementResponse> = {}): MovementResponse {
  return {
    sample: false,
    date: '2026-11-19',
    week_number: 47,
    steps: 6240,
    step_goal: 8000,
    percent_of_goal: 78,
    distance_m: 4600,
    floors: null,
    week_steps: [
      { date: '2026-11-16', value: 9120 },
      { date: '2026-11-17', value: 7480 },
      { date: '2026-11-18', value: 8360 },
      { date: '2026-11-19', value: 6240 },
      { date: '2026-11-20', value: null },
      { date: '2026-11-21', value: null },
      { date: '2026-11-22', value: null },
    ],
    week_average_steps: 7800,
    intensity: { week_start: '2026-11-16', moderate_min: 52, vigorous_min: 22, total_min: 96, goal_min: 150, days: [] },
    calories: { active_kcal: 412, resting_kcal: 1780, total_kcal: 2192 },
    ...over,
  };
}

export function training(over: Partial<TrainingResponse> = {}): TrainingResponse {
  return {
    sample: false,
    as_of: '2026-11-19T22:20:50+01:00',
    recovery: { started_at: '2026-11-19T18:00:00+01:00', total_s: 82800, remaining_s: 14 * 3600, percent_complete: 61, ready_at: '2026-11-20T09:00:00+01:00' },
    load: {
      seven_day_load: 312,
      target_min: 240,
      target_max: 420,
      vs_target: 'within',
      acute: 312,
      chronic: 290,
      ratio: 1.08,
      series_4w: [
        { date: '2026-11-17', value: 300 },
        { date: '2026-11-18', value: 310 },
        { date: '2026-11-19', value: 312 },
      ],
    },
    balance: {
      aerobic_low: 412,
      aerobic_high: 556,
      anaerobic: 128,
      aerobic_low_target_min: 300,
      aerobic_low_target_max: 600,
      aerobic_high_target_min: 400,
      aerobic_high_target_max: 700,
      anaerobic_target_min: 60,
      anaerobic_target_max: 180,
    },
    status: { status_code: 4, label: null, kind: null, since: '2026-11-05', since_is_lower_bound: false, fitness_trend_code: 1, load_trend_code: 2, as_of: '2026-11-19T07:00:00+01:00' },
    vo2max: {
      value: 46.2,
      maximal_met: 13.2,
      derived: true,
      measured_at: '2026-11-18T10:00:00+01:00',
      change_3m: 1,
      series_3m: [
        { date: '2026-09-01', value: 45.2 },
        { date: '2026-11-18', value: 46.2 },
      ],
    },
    readiness: null,
    ...over,
  };
}

export function body(over: Partial<BodyResponse> = {}): BodyResponse {
  return {
    sample: false,
    latest_weight: { value: 80.8, at: '2026-11-17T07:10:00+01:00' },
    latest_body_fat: null,
    latest_waist: { value: 86, at: '2026-11-03T07:20:00+01:00' },
    weight_change_30d_kg: -1.4,
    weights: [
      { time: '2026-10-27T07:15:00+01:00', value: 81.9 },
      { time: '2026-11-03T07:20:00+01:00', value: 81.3 },
      { time: '2026-11-17T07:10:00+01:00', value: 80.8 },
    ],
    weight_trend: [
      { time: '2026-10-27T07:15:00+01:00', value: 81.9 },
      { time: '2026-11-03T07:20:00+01:00', value: 81.6 },
      { time: '2026-11-17T07:10:00+01:00', value: 81.1 },
    ],
    entries: [
      { id: 3, measured_at: '2026-11-17T07:10:00+01:00', weight_kg: 80.8, body_fat_pct: null, waist_cm: null },
      { id: 2, measured_at: '2026-11-03T07:20:00+01:00', weight_kg: 81.3, body_fat_pct: null, waist_cm: 86 },
    ],
    ...over,
  };
}

export function calendar(): CalendarResponse {
  const day = (date: string, kinds: string[], extra: Record<string, number | null> = {}) => ({
    date,
    activities: kinds.map((kind, i) => ({
      id: `${date}-${i}`,
      kind,
      sport_name: null,
      sub_sport_name: null,
      start: `${date}T18:05:00+01:00`,
      date,
      duration_s: 2520,
      distance_m: kind === 'strength' ? null : 6400,
      avg_pace_s_per_km: null,
      avg_hr_bpm: null,
      max_hr_bpm: null,
      calories_kcal: null,
      ascent_m: null,
      sets: kind === 'strength' ? 18 : null,
      volume_kg: null,
      exercises: null,
    })),
    steps: 8360,
    sleep_duration_s: 7 * 3600 + 720,
    sleep_score: 81,
    resting_hr_bpm: 51,
    energy_high: 94,
    energy_low: 38,
    ...extra,
  });
  const days = Array.from({ length: 30 }, (_, i) => `2026-11-${String(i + 1).padStart(2, '0')}`).map((d) => {
    if (d === '2026-11-03') return day(d, ['strength']);
    if (d === '2026-11-05') return day(d, ['run']);
    if (d === '2026-11-10') return day(d, ['run', 'strength']);
    if (d === '2026-11-18') return day(d, ['strength']);
    return day(d, [], { steps: null, sleep_duration_s: null, sleep_score: null, resting_hr_bpm: null, energy_high: null, energy_low: null });
  });
  return { sample: false, month: '2026-11', days, totals: { activities: 5, strength_sessions: 3, cardio_sessions: 2 } };
}
