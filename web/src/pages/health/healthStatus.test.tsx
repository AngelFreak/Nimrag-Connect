import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, within } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HealthMetricKey } from '../../api/generated/HealthMetricKey';
import type { HealthMetricStatus } from '../../api/generated/HealthMetricStatus';
import type { HealthState } from '../../api/generated/HealthState';
import type { HealthStatusHistory } from '../../api/generated/HealthStatusHistory';
import type { HealthStatusResponse } from '../../api/generated/HealthStatusResponse';
import { addDays } from '../../lib/dates';
import { HealthStatusPage } from '../HealthStatusPage';
import { chartFrom, glanceWord, historyChart, metricSentence, stripLayout, summaryLine, summarySub, withoutNeverMeasured } from './healthStatus';

// uPlot needs a real canvas, so the chart is replaced by its name and text alternative.
vi.mock('../../components/SeriesChart', () => ({
  SeriesChart: ({ label, summary }: { label: string; summary: string }) => (
    <div role="img" aria-label={label}>
      <p className="sr-only">{summary}</p>
    </div>
  ),
}));

const DATE = '2026-11-19';

function metric(key: HealthMetricKey, over: Partial<HealthMetricStatus> = {}): HealthMetricStatus {
  const unit = { hrv: 'ms', resting_hr: 'bpm', breathing: 'brpm', spo2: '%' }[key];
  const value = { hrv: 62.4, resting_hr: 52, breathing: 14.5, spo2: 95.6 }[key];
  return { key, unit, value, low: value - 5, high: value + 5, state: 'in_range', nights: 28, nights_needed: 0, ...over };
}

const ALL_IN: HealthMetricStatus[] = (['hrv', 'resting_hr', 'breathing', 'spo2'] as const).map((k) => metric(k));

function response(over: Partial<HealthStatusResponse> = {}): HealthStatusResponse {
  return { sample: false, date: DATE, has_night: true, metrics: ALL_IN, outside: 0, compared: 4, ...over };
}

function historyOf(state: HealthState = 'in_range'): HealthStatusHistory {
  const dates = Array.from({ length: 28 }, (_, i) => addDays(chartFrom(DATE), i));
  const metrics = (['hrv', 'resting_hr', 'breathing', 'spo2'] as const).map((key) => ({
    key,
    unit: ALL_IN.find((m) => m.key === key)!.unit,
    days: dates.map((date, i) => ({ date, value: 50 + (i % 3), low: i < 14 ? null : 45, high: i < 14 ? null : 56, state: i === 27 ? state : ('in_range' as HealthState) })),
  }));
  return { sample: false, from: dates[0]!, to: DATE, metrics };
}

describe('the words of the page', () => {
  it('summarises how many values are outside the usual range', () => {
    expect(summaryLine(response())).toBe('All in your usual range');
    expect(summaryLine(response({ outside: 1 }))).toBe('1 value outside your usual range');
    expect(summaryLine(response({ outside: 2 }))).toBe('2 values outside your usual range');
    // Only some values could be compared.
    expect(summaryLine(response({ compared: 3 }))).toBe('All 3 compared values are in your usual range');
    expect(summaryLine(response({ has_night: false }))).toBe('No night recorded');
  });

  it('says it is still learning when nothing has a range yet', () => {
    const calibrating = ALL_IN.map((m) => ({ ...m, state: 'calibrating' as const, low: null, high: null, nights: 10, nights_needed: 4 }));
    const r = response({ metrics: calibrating, compared: 0 });
    expect(summaryLine(r)).toBe('Still learning your usual range');
    expect(summarySub(r)).toBe('4 values need up to 4 more nights before a usual range can be shown.');
    expect(glanceWord(r)).toEqual({ text: 'Calibrating', state: 'calibrating' });
    expect(summaryLine(response({ metrics: ALL_IN.map((m) => ({ ...m, state: 'no_reading' as const, value: null })), compared: 0 }))).toBe('Nothing measured last night');
  });

  it('leaves out only the values with no reading tonight and none before', () => {
    const never = metric('spo2', { state: 'no_reading', value: null, low: null, high: null, nights: 0 });
    const hadBefore = metric('breathing', { state: 'no_reading', value: null, low: null, high: null, nights: 3 });
    const calibrating = metric('hrv', { state: 'calibrating', low: null, high: null, nights: 0 });
    const r = withoutNeverMeasured(response({ metrics: [calibrating, hadBefore, never, metric('resting_hr')], compared: 1 }));
    expect(r.metrics.map((m) => m.key)).toEqual(['hrv', 'breathing', 'resting_hr']);
    expect(withoutNeverMeasured(response({ metrics: [never] })).metrics).toEqual([]);
    // The words count what is left.
    expect(summarySub(withoutNeverMeasured(response({ metrics: [metric('hrv'), never], compared: 1 })))).toBe('1 of 1 values compared with your last 28 nights.');
  });

  it('names what was not measured', () => {
    const r = response({ metrics: [...ALL_IN.slice(0, 3), metric('spo2', { state: 'no_reading', value: null, low: null, high: null })], compared: 3 });
    expect(summarySub(r)).toBe('Blood oxygen not measured last night.');
  });

  it('gives each value one sentence with its range and place', () => {
    expect(metricSentence(metric('hrv'))).toBe('Overnight HRV: 62.4 ms, inside your usual range of 57.4 to 67.4 ms.');
    expect(metricSentence(metric('spo2', { state: 'below' }))).toBe('Blood oxygen asleep: 95.6%, below your usual range of 90.6% to 100.6%.');
    expect(metricSentence(metric('breathing', { state: 'above' }))).toContain('14.5 breaths a minute, above your usual range of 9.5 to 19.5 breaths a minute');
    expect(metricSentence(metric('hrv', { state: 'calibrating', low: null, high: null, nights_needed: 1 }))).toBe('Overnight HRV: 62.4 ms. Calibrating, 1 more night needed for a usual range.');
    expect(metricSentence(metric('hrv', { state: 'no_reading', value: null }))).toBe('Overnight HRV: not measured last night.');
  });

  it('gives the Today card a short word', () => {
    expect(glanceWord(response())).toEqual({ text: 'In range', state: 'in_range' });
    expect(glanceWord(response({ outside: 2 }))).toEqual({ text: '2 outside', state: 'above' });
    expect(glanceWord(response({ has_night: false }))).toEqual({ text: 'No night yet', state: 'no_reading' });
  });
});

describe('the strip and the chart', () => {
  it('keeps the range and the value inside the bar, with the value at its place', () => {
    const inside = stripLayout(metric('hrv'))!;
    expect(inside.lo).toBeGreaterThan(0);
    expect(inside.hi).toBeLessThan(1);
    expect(inside.at).toBeGreaterThan(inside.lo);
    expect(inside.at).toBeLessThan(inside.hi);
    const above = stripLayout(metric('hrv', { value: 90 }))!;
    expect(above.at).toBeGreaterThan(above.hi);
    expect(above.at).toBeLessThanOrEqual(1);
    const below = stripLayout(metric('hrv', { value: 20 }))!;
    expect(below.at).toBeLessThan(below.lo);
    expect(below.at).toBeGreaterThanOrEqual(0);
    expect(stripLayout(metric('hrv', { low: null, high: null }))).toBeNull();
    expect(stripLayout(metric('hrv', { value: null }))).toBeNull();
  });

  it('marks the nights outside the band and tells the count in words', () => {
    const h = historyOf('above').metrics[0]!;
    const c = historyChart(h, 'Overnight HRV');
    expect(c.measured).toBe(28);
    expect(c.hasBand).toBe(true);
    expect(c.markers.filter((m) => m !== null)).toHaveLength(1);
    expect(c.markers[27]).toEqual({ shape: 'dot', color: 'var(--warn)' });
    expect(c.band.lo.slice(0, 14).every((v) => v === null)).toBe(true);
    expect(c.summary).toBe('Overnight HRV over 28 nights from Oct 23 to Nov 19, 1 outside the usual range.');
    expect(c.xLabels).toHaveLength(4);
  });

  it('has no band while every night is still calibrating', () => {
    const h = historyOf().metrics[0]!;
    const c = historyChart({ ...h, days: h.days.map((d) => ({ ...d, low: null, high: null, state: 'calibrating' })) }, 'Overnight HRV');
    expect(c.hasBand).toBe(false);
  });
});

function mockApi(status: HealthStatusResponse | null, history: HealthStatusHistory | null = historyOf()) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      const body = url.startsWith('/api/health-status/history') ? history : url.startsWith('/api/health-status') ? status : null;
      if (body === null) return new Response(JSON.stringify({ error: 'no data' }), { status: 500, headers: { 'content-type': 'application/json' } });
      return new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' } });
    }),
  );
}

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/health/status']}>
        <Routes>
          <Route path="/health/status" element={<HealthStatusPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.unstubAllGlobals());

describe('Health status page', () => {
  it('shows each value with its usual range, its place and a 4-week chart, and the plain note', async () => {
    mockApi(response({ outside: 1, metrics: [metric('hrv'), metric('resting_hr', { state: 'above', value: 60 }), metric('breathing'), metric('spo2')] }));
    renderPage();
    expect(await screen.findByText('1 value outside your usual range')).toBeInTheDocument();
    const hrv = document.querySelector<HTMLElement>('[data-metric="hrv"]')!;
    expect(within(hrv).getByText('62.4 ms')).toBeInTheDocument();
    expect(within(hrv).getByText('Usual 57.4 to 67.4 ms')).toBeInTheDocument();
    expect(within(hrv).getByText('In range')).toBeInTheDocument();
    expect(within(hrv).getByRole('img', { name: /^Overnight HRV: 62.4 ms, inside your usual range of 57.4 to 67.4 ms\.$/ })).toBeInTheDocument();
    const hr = document.querySelector<HTMLElement>('[data-metric="resting_hr"]')!;
    expect(within(hr).getByText('60 bpm')).toBeInTheDocument();
    expect(within(hr).getByText('Above')).toBeInTheDocument();
    expect(await within(hr).findByRole('img', { name: 'Last 4 weeks' })).toBeInTheDocument();
    expect(screen.getByText(/It is not a medical assessment/)).toBeInTheDocument();
  });

  it('says "All in your usual range" when nothing is outside', async () => {
    mockApi(response());
    renderPage();
    expect(await screen.findByText('All in your usual range')).toBeInTheDocument();
  });

  it('shows how many nights are still needed while calibrating, and no range', async () => {
    const calibrating = ALL_IN.map((m) => ({ ...m, state: 'calibrating' as const, low: null, high: null, nights: 11, nights_needed: 3 }));
    mockApi(response({ metrics: calibrating, compared: 0 }));
    renderPage();
    expect(await screen.findByText('Still learning your usual range')).toBeInTheDocument();
    const hrv = document.querySelector<HTMLElement>('[data-metric="hrv"]')!;
    expect(within(hrv).getByText('Calibrating')).toBeInTheDocument();
    expect(within(hrv).getByText('11 of 14 nights so far. 3 more nights needed before a usual range is shown.')).toBeInTheDocument();
    expect(within(hrv).getByRole('progressbar', { name: '11 of 14 nights collected' })).toBeInTheDocument();
    expect(within(hrv).queryByText(/^Usual/)).not.toBeInTheDocument();
  });

  it('says a value was not measured instead of showing a zero', async () => {
    // It has been measured on earlier nights: it stays, as "No reading".
    mockApi(response({ metrics: [...ALL_IN.slice(0, 3), metric('spo2', { state: 'no_reading', value: null, low: null, high: null, nights: 9, nights_needed: 5 })], compared: 3 }));
    renderPage();
    const o2 = await waitForCard('spo2');
    expect(within(o2).getByText('No reading')).toBeInTheDocument();
    expect(within(o2).queryByText('unavailable')).not.toBeInTheDocument();
    expect(within(o2).getByText('Not measured last night')).toBeInTheDocument();
  });

  it('leaves out a value that has never been measured', async () => {
    mockApi(response({ metrics: [...ALL_IN.slice(0, 3), metric('spo2', { state: 'no_reading', value: null, low: null, high: null, nights: 0, nights_needed: 14 })], compared: 3 }));
    renderPage();
    // The line under the summary counts what is shown.
    expect(await screen.findByText(/3 of 3 values compared/)).toBeInTheDocument();
    expect(document.querySelector('[data-metric="spo2"]')).toBeNull();
    expect(document.querySelectorAll('[data-metric]')).toHaveLength(3);
    expect(screen.queryByText('No reading')).not.toBeInTheDocument();
  });

  it('says so calmly when every value was left out', async () => {
    mockApi(response({ compared: 0, metrics: ALL_IN.map((m) => ({ ...m, state: 'no_reading' as const, value: null, low: null, high: null, nights: 0, nights_needed: 14 })) }));
    renderPage();
    expect(await screen.findByText('No overnight readings yet.')).toBeInTheDocument();
    expect(document.querySelector('[data-metric]')).toBeNull();
  });

  it('has an empty state when no night was recorded', async () => {
    mockApi(response({ has_night: false, compared: 0, metrics: ALL_IN.map((m) => ({ ...m, state: 'no_reading' as const, value: null })) }));
    renderPage();
    expect(await screen.findByText('No night recorded')).toBeInTheDocument();
    expect(screen.getByText('Nothing to compare yet')).toBeInTheDocument();
    expect(document.querySelector('[data-metric]')).toBeNull();
  });

  it('says so when the page cannot load, and when only the chart cannot', async () => {
    mockApi(null);
    renderPage();
    expect(await screen.findByText('Could not load your health status')).toBeInTheDocument();
  });

  it('keeps the values when the 4-week history fails', async () => {
    mockApi(response(), null);
    renderPage();
    expect((await screen.findAllByText('The 4-week chart could not be loaded.')).length).toBe(4);
    expect(screen.getByText('All in your usual range')).toBeInTheDocument();
  });
});

async function waitForCard(key: string): Promise<HTMLElement> {
  await screen.findByText(/values? (outside|compared)|All .* usual range|compared values/);
  return document.querySelector<HTMLElement>(`[data-metric="${key}"]`)!;
}
