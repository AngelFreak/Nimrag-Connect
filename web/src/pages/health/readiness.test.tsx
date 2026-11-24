import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ReadinessDay } from '../../api/generated/ReadinessDay';
import type { ReadinessFactor } from '../../api/generated/ReadinessFactor';
import type { ReadinessHistory } from '../../api/generated/ReadinessHistory';
import type { ReadinessResponse } from '../../api/generated/ReadinessResponse';
import type { StatusExplain } from '../../api/generated/StatusExplain';
import { browserToday, addDays } from '../../lib/dates';
import { ReadinessPage } from '../ReadinessPage';
import { ReadinessCard } from './ReadinessCard';
import { StatusExplanation } from './StatusExplanation';
import { LEVEL_COLOR, factorSub, factorValue, fmtNum, historyChart, historyWindow } from './readiness';

// Rendering of the readiness page, its card and the training-status explanation, with and without
// data; and the small helpers the page is built from.

const TODAY = browserToday();
const day = (n: number) => addDays(TODAY, -n);

function f(key: ReadinessFactor['key'], value: number | null, unit: string, rating: ReadinessFactor['rating'], weight: number, note: string | null = null): ReadinessFactor {
  return { key, value, unit, rating, weight, note };
}

const FULL: ReadinessResponse = {
  sample: false,
  date: TODAY,
  score: 78,
  level: 'high',
  headline_key: 'high_sleep',
  headline: 'Ready for a solid session',
  sentence: 'You are ready for a hard session, though your sleep over the past week is a little behind.',
  factors: [
    f('sleep', 85, 'score', 'good', 25),
    f('recovery', 0, 'h', 'very_good', 20),
    f('hrv', 60, 'ms', 'good', 20, 'Balanced range 50–70 ms'),
    f('load', 250, 'load', 'good', 15, 'Target range 200–400'),
    f('sleep_history', 73.9, 'score', 'moderate', 10),
    f('stress', 30, 'stress', 'good', 10),
  ],
  present: 6,
  data_days: 7,
  unavailable: null,
};

const NONE: ReadinessResponse = {
  sample: false,
  date: TODAY,
  score: null,
  level: null,
  headline_key: null,
  headline: null,
  sentence: null,
  factors: FULL.factors.map((x) => ({ ...x, value: null, rating: null, note: null })),
  present: 0,
  data_days: 0,
  unavailable: 'Readiness needs at least 3 days of data. Sync your watch for a few days.',
};

const hday = (n: number, score: number | null, level: ReadinessDay['level']): ReadinessDay => ({ date: day(n), score, level });
function historyOf(days: ReadinessDay[], average: number | null): ReadinessHistory {
  return { sample: false, from: days[0]?.date ?? day(6), to: days[days.length - 1]?.date ?? TODAY, days, average };
}

const EXPLAIN: StatusExplain = {
  sample: false,
  status: { code: 7, label: 'Productive', kind: 'productive' },
  sentence_key: 'productive_working',
  sentence: 'Your load is in the right range and your VO2 max is rising, so this training is working.',
  meaning: { title: 'Productive', text: 'Your recent training is the right size and your fitness is going up.' },
  factors: [
    { key: 'vo2max', label: 'VO2 max', value: 50.4, unit: 'ml/kg/min', change: 1.4, word: 'Rising', detail: null },
    { key: 'load', label: '7-day load', value: 250, unit: 'load', change: null, word: 'Optimal', detail: 'Target range 200–400' },
    { key: 'hrv', label: 'HRV, 7-day average', value: 60, unit: 'ms', change: null, word: 'Balanced', detail: 'Balanced range 50–70 ms' },
  ],
  focus: [
    { key: 'aerobic_low', label: 'Easy aerobic', value: 300, target_min: 200, target_max: 400, state: 'in_range' },
    { key: 'aerobic_high', label: 'Hard aerobic', value: 250, target_min: 150, target_max: 300, state: 'in_range' },
    { key: 'anaerobic', label: 'Anaerobic', value: 20, target_min: 60, target_max: 180, state: 'short' },
  ],
  short_of: 'anaerobic',
};

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

function renderAt(path: string, ui: React.ReactNode = <ReadinessPage />) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/training/readiness" element={ui} />
          <Route path="*" element={ui} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('helpers', () => {
  it('writes numbers as the API has them, without rounding further', () => {
    expect(fmtNum(73.9)).toBe('73.9');
    expect(fmtNum(85)).toBe('85');
    expect(fmtNum(0)).toBe('0');
    expect(fmtNum(null)).toBeNull();
    expect(fmtNum(Number.NaN)).toBeNull();
  });

  it('adds units to hours and milliseconds only', () => {
    expect(factorValue({ value: 0, unit: 'h' })).toBe('0 h');
    expect(factorValue({ value: 60, unit: 'ms' })).toBe('60 ms');
    expect(factorValue({ value: 85, unit: 'score' })).toBe('85');
    expect(factorValue({ value: null, unit: 'ms' })).toBeNull();
  });

  it('says what a factor row shows under its name', () => {
    expect(factorSub(FULL.factors[2]!)).toBe('60 ms · Balanced 50–70 ms');
    expect(factorSub(FULL.factors[0]!)).toBe('85 · Last night');
    expect(factorSub(NONE.factors[0]!)).toBe('Not available yet');
  });

  it('windows end today and are 7, 28 and 84 days long', () => {
    expect(historyWindow('7d', '2026-11-19')).toEqual({ from: '2026-11-13', to: '2026-11-19' });
    expect(historyWindow('4w', '2026-11-19')).toEqual({ from: '2026-10-23', to: '2026-11-19' });
    expect(historyWindow('12w', '2026-11-19')).toEqual({ from: '2026-08-28', to: '2026-11-19' });
  });

  it('colours the bars by level and leaves a day without an estimate as a gap', () => {
    const days = [hday(2, 40, 'low'), hday(1, null, null), hday(0, 96, 'prime')];
    const c = historyChart(days, 68);
    expect(c.values).toEqual([40, null, 96]);
    expect(c.colors).toEqual([LEVEL_COLOR.low, undefined, LEVEL_COLOR.prime]);
    expect(c.scored).toBe(2);
    expect(c.summary).toContain('2 of 3 days with an estimate, average 68');
    expect(c.summary).toContain('highest 96');
    expect(c.summary).toContain('lowest 40');
    expect(historyChart([hday(0, null, null)], null).summary).toBe('No readiness estimate in this period.');
  });

  it('labels a week by weekday and a longer period by its first and last date', () => {
    const week = historyChart(Array.from({ length: 7 }, (_, i) => hday(6 - i, 50, 'moderate')), 50);
    expect(week.axis).toEqual([]);
    expect(week.labels.every((l) => l.length === 3)).toBe(true);
    const month = historyChart(Array.from({ length: 28 }, (_, i) => hday(27 - i, 50, 'moderate')), 50);
    expect(month.axis).toHaveLength(2);
    expect(month.labels.every((l) => l === '')).toBe(true);
  });
});

describe('the readiness page', () => {
  it('shows the score, level, the estimate label, the words and the six factors', async () => {
    mockApi({ '/api/readiness/history': historyOf([hday(1, 70, 'moderate'), hday(0, 78, 'high')], 74), '/api/readiness': FULL });
    renderAt('/training/readiness');
    expect(await screen.findByText('Ready for a solid session')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Readiness estimate 78 out of 100' })).toBeInTheDocument();
    expect(screen.getByText('High')).toBeInTheDocument();
    expect(screen.getByText('Nimrag estimate — not Garmin’s Training Readiness')).toBeInTheDocument();
    expect(screen.getByText(/your sleep over the past week is a little behind/)).toBeInTheDocument();

    const list = screen.getByRole('list', { name: 'What the estimate is made from' });
    const rows = within(list).getAllByRole('listitem');
    expect(rows).toHaveLength(6);
    expect(within(rows[0]!).getByText('Sleep')).toBeInTheDocument();
    expect(within(rows[0]!).getByText('Good')).toBeInTheDocument();
    expect(within(rows[1]!).getByText('Recovered')).toBeInTheDocument();
    expect(within(rows[1]!).getByText('0 h · Time left')).toBeInTheDocument();
    expect(within(rows[4]!).getByText('73.9 · 7-night average')).toBeInTheDocument();
    expect(within(rows[4]!).getByText('Fair')).toBeInTheDocument();
    expect(within(rows[2]!).getByRole('link')).toHaveAttribute('href', '/hrv');
    expect(screen.getByText('6 of 6 factors available', { exact: false })).toBeInTheDocument();
  });

  it('says why there is no estimate and marks every factor as not available', async () => {
    mockApi({ '/api/readiness/history': historyOf([hday(0, null, null)], null), '/api/readiness': NONE });
    renderAt('/training/readiness');
    expect(await screen.findByText('No estimate yet')).toBeInTheDocument();
    expect(screen.getByText(/needs at least 3 days of data/)).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Readiness estimate unavailable' })).toBeInTheDocument();
    expect(screen.getAllByText('Not available')).toHaveLength(6);
    expect(await screen.findByText('No estimate yet in this period.')).toBeInTheDocument();
  });

  it('asks for the window of the period the person picks', async () => {
    const calls = mockApi({ '/api/readiness/history': historyOf([hday(1, 70, 'moderate'), hday(0, 78, 'high')], 74), '/api/readiness': FULL });
    renderAt('/training/readiness');
    await screen.findByText('Ready for a solid session');
    await screen.findByText(/2 of 2 days with an estimate/);
    expect(calls.some((c) => c.includes(`/api/readiness/history?from=${addDays(TODAY, -27)}&to=${TODAY}`))).toBe(true);
    await userEvent.click(screen.getByRole('button', { name: '12w' }));
    await vi.waitFor(() => expect(calls.some((c) => c.includes(`from=${addDays(TODAY, -83)}&to=${TODAY}`))).toBe(true));
  });

  it('shows an error with a way to retry', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ error: 'nope' }), { status: 500, headers: { 'content-type': 'application/json' } })));
    renderAt('/training/readiness');
    expect(await screen.findByText('Could not load the readiness estimate')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
  });
});

describe('the readiness card', () => {
  it('links to the page with the level and the headline', async () => {
    mockApi({ '/api/readiness': FULL });
    renderAt('/training', <ReadinessCard />);
    const link = await screen.findByRole('link', { name: /Readiness, Nimrag estimate/ });
    expect(link).toHaveAttribute('href', '/training/readiness');
    expect(await within(link).findByText('High')).toBeInTheDocument();
    expect(within(link).getByText('Ready for a solid session')).toBeInTheDocument();
  });

  it('says there is no estimate yet when the server has none', async () => {
    mockApi({ '/api/readiness': NONE });
    renderAt('/training', <ReadinessCard />);
    const link = await screen.findByRole('link', { name: /Readiness, Nimrag estimate/ });
    expect(await within(link).findByText('No estimate yet')).toBeInTheDocument();
    expect(within(link).getByText(/at least 3 days/)).toBeInTheDocument();
  });
});

describe('the training status explanation', () => {
  it('gives the sentence, the figures, the load focus and a paragraph that opens on demand', async () => {
    mockApi({ '/api/training/explain': EXPLAIN });
    renderAt('/training/status', <StatusExplanation />);
    expect(await screen.findByText(/VO2 max is rising, so this training is working/)).toBeInTheDocument();

    const figures = screen.getByRole('list', { name: 'Figures behind the status' });
    const rows = within(figures).getAllByRole('listitem');
    expect(rows).toHaveLength(3);
    expect(within(rows[0]!).getByText('50.4 ml/kg/min · +1.4 over 4 weeks')).toBeInTheDocument();
    expect(within(rows[0]!).getByText('Rising')).toBeInTheDocument();
    expect(within(rows[0]!).getByRole('link')).toHaveAttribute('href', '/metrics/vo2_max');
    expect(within(rows[1]!).getByText('250 · Target range 200–400')).toBeInTheDocument();
    expect(within(rows[1]!).getByText('Optimal')).toBeInTheDocument();
    expect(within(rows[2]!).getByText('60 ms · Balanced range 50–70 ms')).toBeInTheDocument();

    expect(screen.getByText('Anaerobic is furthest short of its target.')).toBeInTheDocument();
    const focus = within(screen.getByRole('list', { name: 'Load focus by intensity' })).getAllByRole('listitem');
    expect(within(focus[2]!).getByText('Short of target')).toBeInTheDocument();
    expect(within(focus[0]!).getByText('In range')).toBeInTheDocument();

    const toggle = screen.getByRole('button', { name: 'What this status means' });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getByText(/fitness is going up/)).not.toBeVisible();
    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText(/fitness is going up/)).toBeVisible();
  });

  it('keeps to the sentence and the paragraph in the brief form', async () => {
    mockApi({ '/api/training/explain': EXPLAIN });
    renderAt('/training', <StatusExplanation brief />);
    await screen.findByText(/this training is working/);
    expect(screen.queryByRole('list')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'What this status means' })).toBeInTheDocument();
  });

  it('says there is nothing to explain without a status, and when the figures are missing', async () => {
    mockApi({
      '/api/training/explain': {
        ...EXPLAIN,
        status: null,
        sentence_key: null,
        sentence: null,
        meaning: null,
        factors: EXPLAIN.factors.map((x) => ({ ...x, value: null, change: null, word: null, detail: null })),
        focus: [],
        short_of: null,
      } satisfies StatusExplain,
    });
    renderAt('/training/status', <StatusExplanation />);
    expect(await screen.findByText(/nothing to explain/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'What this status means' })).not.toBeInTheDocument();
    expect(screen.getAllByText('Not available yet')).toHaveLength(3);
    expect(screen.queryByText(/Load focus/)).not.toBeInTheDocument();
  });

  it('says so when the explanation cannot be loaded', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ error: 'x' }), { status: 500, headers: { 'content-type': 'application/json' } })));
    renderAt('/training/status', <StatusExplanation />);
    expect(await screen.findByText('The explanation could not be loaded.')).toBeInTheDocument();
  });
});
