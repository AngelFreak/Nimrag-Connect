import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { RaceHistoryPoint } from '../../api/generated/RaceHistoryPoint';
import type { RacePrediction } from '../../api/generated/RacePrediction';
import type { RacePredictionsResponse } from '../../api/generated/RacePredictionsResponse';
import { RaceCard } from './RaceCard';
import { RacePredictorPage } from './RacePredictorPage';
import { basisText, fmtChange, fmtPace, fmtRaceTime, pointsFor, raceChart } from './model';

// uPlot draws on a canvas, which jsdom does not have.
vi.mock('uplot', () => ({
  default: class {
    width = 320;
    setSize() {}
    destroy() {}
  },
}));

// The race predictor page: the way times are written, the chart's data, and the page with and
// without an estimate.

const pred = (d: number, seconds: number, change: number | null = null): RacePrediction => ({ distance_m: d, seconds, pace_s_per_km: seconds / (d / 1000), change_4w_s: change });

const NOW: RacePrediction[] = [pred(5000, 1497, -42), pred(10000, 3101, 65), pred(21097.5, 6895, 0), pred(42195, 14449, null)];

const point = (date: string, five: number): RaceHistoryPoint => ({ date, predictions: [pred(5000, five), pred(10000, five * 2.1)] });
const HISTORY: RaceHistoryPoint[] = ['2026-10-04', '2026-10-11', '2026-10-18', '2026-10-25', '2026-11-01', '2026-11-08', '2026-11-15'].map((d, i) => point(d, 1560 - i * 10));

const FULL: RacePredictionsResponse = { sample: false, as_of: '2026-11-19', basis: 'both', vo2max: 50.4, predictions: NOW, history: HISTORY };
const EMPTY: RacePredictionsResponse = { sample: false, as_of: '2026-11-19', basis: null, vo2max: null, predictions: [], history: [] };

function mockApi(body: unknown, status = 200) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })),
  );
}

function renderPage(ui: React.ReactNode = <RacePredictorPage />) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>{ui}</MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('formatting', () => {
  it('writes m:ss under an hour and h:mm:ss from an hour on', () => {
    expect(fmtRaceTime(59)).toBe('0:59');
    expect(fmtRaceTime(1197)).toBe('19:57');
    expect(fmtRaceTime(3599)).toBe('59:59');
    expect(fmtRaceTime(3600)).toBe('1:00:00');
    expect(fmtRaceTime(5495)).toBe('1:31:35');
    expect(fmtRaceTime(11449)).toBe('3:10:49');
  });

  it('writes the pace per km in whole seconds', () => {
    expect(fmtPace(239.4)).toBe('3:59 /km');
    expect(fmtPace(1497 / 5)).toBe('4:59 /km');
  });

  it('writes the change with a real minus sign and a tone', () => {
    expect(fmtChange(-42)).toEqual({ text: '−0:42 vs 4 weeks ago', tone: 'faster' });
    expect(fmtChange(65)).toEqual({ text: '+1:05 vs 4 weeks ago', tone: 'slower' });
    expect(fmtChange(-3725)).toEqual({ text: '−1:02:05 vs 4 weeks ago', tone: 'faster' });
    expect(fmtChange(0)).toEqual({ text: 'No change in 4 weeks', tone: 'same' });
    expect(fmtChange(null)).toBeNull();
  });

  it('says what the estimate is based on', () => {
    expect(basisText('both', 50.4)).toBe('Based on your best efforts in the last 6 weeks and your VO2 max of 50.4.');
    expect(basisText('efforts', null)).toBe('Based on your best efforts in the last 6 weeks.');
    expect(basisText('vo2max', 50)).toContain('No run with GPS in the last 6 weeks');
    expect(basisText(null, null)).toBe('');
  });
});

describe('the chart data', () => {
  it('shows the last four weekly points for 4 weeks and all for 6 months', () => {
    expect(pointsFor(HISTORY, '4w').map((p) => p.date)).toEqual(['2026-10-25', '2026-11-01', '2026-11-08', '2026-11-15']);
    expect(pointsFor(HISTORY, '6m')).toHaveLength(7);
  });

  it('counts days from the first point and gives the time in minutes', () => {
    const c = raceChart(HISTORY, 5000);
    expect(c.x).toEqual([0, 7, 14, 21, 28, 35, 42]);
    expect(c.y[0]).toBe(1560 / 60);
    expect(c.count).toBe(7);
    expect(c.xLabels.map((l) => l.value)).toEqual([0, 14, 28, 42]);
    expect(c.summary).toBe('5 km: 26:00 on 4 Oct, 25:00 on 15 Nov. Fastest 25:00.');
  });

  it('has no chart for a distance without points', () => {
    const c = raceChart(HISTORY, 42195);
    expect(c.count).toBe(0);
    expect(c.summary).toBe('No Marathon estimate in this period.');
  });
});

describe('the page', () => {
  it('lists the four distances with time, pace and the change', async () => {
    mockApi(FULL);
    renderPage();
    const list = await screen.findByRole('list', { name: 'Predicted race times' });
    const rows = within(list).getAllByRole('listitem');
    expect(rows).toHaveLength(4);
    expect(within(rows[0]!).getByRole('heading', { name: '5 km' })).toBeInTheDocument();
    expect(within(rows[0]!).getByText('24:57')).toBeInTheDocument();
    expect(within(rows[0]!).getByText('4:59 /km')).toBeInTheDocument();
    expect(within(rows[0]!).getByText('−0:42 vs 4 weeks ago')).toBeInTheDocument();
    expect(within(rows[1]!).getByText('+1:05 vs 4 weeks ago')).toBeInTheDocument();
    expect(within(rows[2]!).getByText('1:54:55')).toBeInTheDocument();
    expect(within(rows[2]!).getByText('No change in 4 weeks')).toBeInTheDocument();
    expect(within(rows[3]!).getByText('Nothing to compare with 4 weeks ago')).toBeInTheDocument();
    expect(screen.getByText('Nimrag estimate — not Garmin’s Race Predictor')).toBeInTheDocument();
    expect(screen.getByText(/your VO2 max of 50\.4/)).toBeInTheDocument();
  });

  it('picks the distance and the period for the chart', async () => {
    mockApi(FULL);
    const user = userEvent.setup();
    renderPage();
    await screen.findByRole('list', { name: 'Predicted race times' });
    const five = screen.getByRole('button', { name: '5 km' });
    expect(five).toHaveAttribute('aria-pressed', 'true');
    await user.click(screen.getByRole('button', { name: '10 km' }));
    expect(screen.getByRole('button', { name: '10 km' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: '5 km' })).toHaveAttribute('aria-pressed', 'false');
    await user.click(screen.getByRole('button', { name: 'Marathon' }));
    expect(screen.getByText('No Marathon estimate in this period.')).toBeInTheDocument();
  });

  it('asks for a run with GPS when there is no estimate', async () => {
    mockApi(EMPTY);
    renderPage();
    expect(await screen.findByText('No estimate yet')).toBeInTheDocument();
    expect(screen.getByText('A run with GPS in the last 6 weeks is needed.')).toBeInTheDocument();
    expect(screen.queryByRole('list', { name: 'Predicted race times' })).not.toBeInTheDocument();
  });

  it('shows an error with a retry', async () => {
    mockApi({ error: 'nope' }, 500);
    renderPage();
    expect(await screen.findByText('The race predictions could not be loaded')).toBeInTheDocument();
  });
});

describe('the card on the Training page', () => {
  it('links to the page with the 5 km time', async () => {
    mockApi(FULL);
    renderPage(<RaceCard />);
    expect(await screen.findByText('5 km in 24:57')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Race predictor/ })).toHaveAttribute('href', '/training/race-predictor');
  });

  it('says what is needed without an estimate', async () => {
    mockApi(EMPTY);
    renderPage(<RaceCard />);
    expect(await screen.findByText('A run with GPS in the last 6 weeks is needed.')).toBeInTheDocument();
  });
});
