import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BodyPage } from '../BodyPage';
import { CalendarPage } from '../CalendarPage';
import { MovementPage } from '../MovementPage';
import { TrainingPage } from '../TrainingPage';
import { body, calendar, json, mockApi, movement, renderAt, TODAY, training } from './testUtils';

// uPlot draws on a canvas, which jsdom does not have.
vi.mock('uplot', () => ({
  default: class {
    width = 320;
    setSize() {}
    destroy() {}
  },
}));

beforeEach(() => {
  vi.useRealTimers();
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe('MovementPage (3i)', () => {
  it('shows steps against the goal, the week chart, intensity and calories from the API', async () => {
    const { calls } = mockApi({ '/api/today': TODAY, '/api/movement/2026-11-19': movement({ sample: true }) });
    renderAt(<MovementPage />, '/movement', '/movement/:date?');
    expect(await screen.findByText('6,240')).toBeInTheDocument();
    expect(calls.some((c) => c.url === '/api/movement/2026-11-19')).toBe(true);
    expect(screen.getByText('Goal 8,000')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: '6,240 of 8,000 steps, 78 percent' })).toBeInTheDocument();
    expect(screen.getByText('4.6 km')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: /Steps per day this week: Monday 9,120, Tuesday 7,480, Wednesday 8,360, Thursday 6,240, Friday no data/ })).toBeInTheDocument();
    expect(screen.getByText('Avg 7,800 a day')).toBeInTheDocument();
    expect(screen.getByText('96 min')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: /96 of 150 intensity minutes: 52 moderate and 44 from vigorous activity, counted double/ })).toBeInTheDocument();
    expect(screen.getByText('412 kcal')).toBeInTheDocument();
    expect(screen.getByText('2,192 kcal')).toBeInTheDocument();
    expect(screen.getByText('Sample data')).toBeInTheDocument();
    expect(screen.getByText(/Thu 19 Nov · week 47/)).toBeInTheDocument();
  });

  it('has no floors row (no data source), and no zero in its place', async () => {
    mockApi({ '/api/today': TODAY, '/api/movement/': movement() });
    renderAt(<MovementPage />, '/movement', '/movement/:date?');
    await screen.findByText('6,240');
    expect(screen.queryByText('Floors climbed')).not.toBeInTheDocument();
  });

  it('missing metrics show "unavailable" and no zero', async () => {
    mockApi({
      '/api/today': TODAY,
      '/api/movement/': movement({
        steps: null,
        percent_of_goal: null,
        distance_m: null,
        week_average_steps: null,
        week_steps: movement().week_steps.map((d) => ({ ...d, value: null })),
        intensity: { week_start: '2026-11-16', moderate_min: null, vigorous_min: null, total_min: null, goal_min: 150, days: [] },
        calories: { active_kcal: null, resting_kcal: null, total_kcal: null },
      }),
    });
    renderAt(<MovementPage />, '/movement', '/movement/:date?');
    await screen.findByText('Steps today');
    await waitFor(() => expect(screen.getAllByText('unavailable').length).toBeGreaterThanOrEqual(8));
    expect(screen.queryByText('0')).not.toBeInTheDocument();
    expect(screen.queryByText(/^0 /)).not.toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Steps today unavailable' })).toBeInTheDocument();
  });

  it('takes the date from the URL and links to the previous and next day', async () => {
    const { calls } = mockApi({ '/api/today': TODAY, '/api/movement/': movement({ date: '2026-11-17', week_number: 47 }) });
    renderAt(<MovementPage />, '/movement/2026-11-17', '/movement/:date?');
    await screen.findByText('6,240');
    expect(calls.some((c) => c.url === '/api/movement/2026-11-17')).toBe(true);
    expect(screen.getByRole('link', { name: 'Previous day' })).toHaveAttribute('href', '/movement/2026-11-16');
    expect(screen.getByRole('link', { name: 'Next day' })).toHaveAttribute('href', '/movement/2026-11-18');
  });

  it('has no "next day" link on today', async () => {
    mockApi({ '/api/today': TODAY, '/api/movement/': movement() });
    renderAt(<MovementPage />, '/movement', '/movement/:date?');
    await screen.findByText('6,240');
    expect(screen.queryByRole('link', { name: 'Next day' })).not.toBeInTheDocument();
  });

  it('rejects a malformed date without calling the API', async () => {
    const { calls } = mockApi({ '/api/today': TODAY });
    renderAt(<MovementPage />, '/movement/nonsense', '/movement/:date?');
    expect(await screen.findByText('That is not a date')).toBeInTheDocument();
    expect(calls.some((c) => c.url.startsWith('/api/movement'))).toBe(false);
  });

  it('shows a loading state, then an error with a retry', async () => {
    let fail = true;
    mockApi({
      '/api/today': TODAY,
      '/api/movement/': () => (fail ? json({ error: 'database busy' }, { status: 500 }) : json(movement())),
    });
    renderAt(<MovementPage />, '/movement/2026-11-19', '/movement/:date?');
    expect(screen.getByRole('status')).toHaveTextContent('Loading');
    expect(await screen.findByRole('alert', {}, { timeout: 4000 })).toHaveTextContent('database busy');
    fail = false;
    await userEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByText('6,240')).toBeInTheDocument();
  });
});

describe('TrainingPage (3j)', () => {
  it('shows recovery, 7-day load against its target, load by intensity and derived VO2 max', async () => {
    mockApi({ '/api/training': training() });
    renderAt(<TrainingPage />, '/training');
    expect(await screen.findByText('Recovery time left')).toBeInTheDocument();
    expect(screen.getByText('14 h')).toBeInTheDocument();
    expect(screen.getByText(/Fully recovered Fri 20 Nov 09:00/)).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Recovery complete, percent 61 out of 100' })).toBeInTheDocument();
    expect(screen.getByText('Target range 240 to 420')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Load of 312 inside the target range of 240 to 420' })).toBeInTheDocument();
    expect(screen.getByText('Inside the target range')).toBeInTheDocument();
    const balance = screen.getByRole('list', { name: 'Load by intensity' });
    expect(within(balance).getByText('Easy aerobic')).toBeInTheDocument();
    expect(within(balance).getByText('556')).toBeInTheDocument();
    expect(screen.getByText('Anaerobic target')).toBeInTheDocument();
    expect(screen.getByText('60 to 180')).toBeInTheDocument();
    expect(screen.getByText('As of Thu 19 Nov')).toBeInTheDocument();
  });

  it('labels VO2 max as derived (max MET x 3.5)', async () => {
    mockApi({ '/api/training': training() });
    renderAt(<TrainingPage />, '/training');
    expect(await screen.findByRole('heading', { name: 'VO2 max (derived)' })).toBeInTheDocument();
    expect(screen.getByText('46.2')).toBeInTheDocument();
    expect(screen.getByText(/derived \(max MET × 3\.5\)/)).toBeInTheDocument();
    expect(screen.getByText('+1.0 in 3 months')).toBeInTheDocument();
  });

  it('shows "Other status" instead of a code when the label is null, and no trend codes', async () => {
    mockApi({ '/api/training': training() });
    renderAt(<TrainingPage />, '/training');
    expect(await screen.findByText('Other status')).toBeInTheDocument();
    expect(screen.getByText(/has no name here yet/)).toBeInTheDocument();
    expect(screen.getByText('Since Thu 5 Nov')).toBeInTheDocument();
    expect(screen.queryByText(/Code \d/)).not.toBeInTheDocument();
    expect(screen.queryByText('Fitness trend')).not.toBeInTheDocument();
    expect(screen.queryByText('Load trend')).not.toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/status code/i);
  });

  it('shows "unavailable" for missing recovery, load, balance, status and VO2 max', async () => {
    mockApi({
      '/api/training': training({
        recovery: { started_at: null, total_s: null, remaining_s: null, percent_complete: null, ready_at: null },
        load: { seven_day_load: null, target_min: null, target_max: null, vs_target: null, acute: null, chronic: null, ratio: null, series_4w: [] },
        balance: null,
        status: { status_code: null, label: null, kind: null, since: null, since_is_lower_bound: false, fitness_trend_code: null, load_trend_code: null, as_of: null },
        vo2max: { value: null, maximal_met: null, derived: true, measured_at: null, change_3m: null, series_3m: [] },
      }),
    });
    renderAt(<TrainingPage />, '/training');
    await screen.findByText('Recovery time left');
    expect(screen.getAllByText('unavailable').length).toBeGreaterThanOrEqual(12);
    expect(screen.queryByText(/Status code/)).not.toBeInTheDocument();
    expect(screen.queryByText(/^0/)).not.toBeInTheDocument();
  });

  it('says "Recovered" when no recovery time is left', async () => {
    mockApi({ '/api/training': training({ recovery: { started_at: null, total_s: 3600, remaining_s: 0, percent_complete: 100, ready_at: null } }) });
    renderAt(<TrainingPage />, '/training');
    expect(await screen.findByText('Recovered')).toBeInTheDocument();
  });

  it('shows an error state when the request fails', async () => {
    mockApi({ '/api/training': () => json({ error: 'nope' }, { status: 400 }) });
    renderAt(<TrainingPage />, '/training');
    expect(await screen.findByRole('alert')).toHaveTextContent('nope');
  });
});

describe('BodyPage (3k)', () => {
  it('shows the latest weight, 30-day change, chart summary, tiles and entries', async () => {
    mockApi({ '/api/today': TODAY, '/api/body': body() });
    renderAt(<BodyPage />, '/body');
    expect(await screen.findByText('80.8 kg')).toBeInTheDocument();
    expect(screen.getByText('Weight · Tue 17 Nov')).toBeInTheDocument();
    expect(screen.getByText(/30-day change: −1\.4 kg/)).toBeInTheDocument();
    expect(screen.getByRole('img', { name: /Weight trend over 90 days/ })).toBeInTheDocument();
    expect(screen.getByText(/3 weigh-ins, from 81.9 kg on Tue 27 Oct to 80.8 kg on Tue 17 Nov/)).toBeInTheDocument();
    expect(screen.getByText('86.0 cm')).toBeInTheDocument();
    // body fat has no entry: unavailable, not 0
    const fat = screen.getByText('Body fat').closest('section') as HTMLElement;
    expect(within(fat).getByText('unavailable')).toBeInTheDocument();
    const list = screen.getByRole('heading', { name: 'Entries' }).closest('section') as HTMLElement;
    expect(within(list).getByText('Tue 17 Nov · 07:10')).toBeInTheDocument();
    expect(within(list).getByText('Weight 81.3 kg · Waist 86.0 cm')).toBeInTheDocument();
  });

  it('shows the latest 10 entries and a "Show all (N)" button for the rest', async () => {
    const many = Array.from({ length: 34 }, (_, i) => ({
      id: 100 - i,
      measured_at: new Date(Date.UTC(2026, 10, 17 - i, 6, 10)).toISOString().replace('Z', '+00:00'),
      weight_kg: 80 + i / 10,
      body_fat_pct: null,
      waist_cm: null,
    }));
    mockApi({ '/api/today': TODAY, '/api/body': body({ entries: many }) });
    renderAt(<BodyPage />, '/body');
    const list = (await screen.findByRole('heading', { name: 'Entries' })).closest('section') as HTMLElement;
    expect(within(list).getAllByRole('listitem')).toHaveLength(10);
    expect(within(list).getAllByRole('button', { name: /^Delete entry/ })).toHaveLength(10);
    await userEvent.click(within(list).getByRole('button', { name: 'Show all (34)' }));
    expect(within(list).getAllByRole('listitem')).toHaveLength(34);
    expect(within(list).queryByRole('button', { name: /Show all/ })).not.toBeInTheDocument();
  });

  it('has no "Show all" button when there are 10 entries or fewer', async () => {
    mockApi({ '/api/today': TODAY, '/api/body': body() });
    renderAt(<BodyPage />, '/body');
    await screen.findByRole('heading', { name: 'Entries' });
    expect(screen.queryByRole('button', { name: /Show all/ })).not.toBeInTheDocument();
  });

  it('shows an empty state with an action when there are no entries', async () => {
    mockApi({
      '/api/today': TODAY,
      '/api/body': body({ latest_weight: null, latest_waist: null, weight_change_30d_kg: null, weights: [], weight_trend: [], entries: [] }),
    });
    renderAt(<BodyPage />, '/body');
    expect(await screen.findByText('No entries yet')).toBeInTheDocument();
    expect(screen.getAllByText('unavailable').length).toBeGreaterThanOrEqual(4);
    await userEvent.click(within(screen.getByText('No entries yet').closest('section') as HTMLElement).getByRole('button', { name: 'Add entry' }));
    expect(screen.getByLabelText('Weight (kg)')).toHaveFocus();
  });

  it('validates the form before sending anything', async () => {
    const { calls } = mockApi({ '/api/today': TODAY, '/api/body': body() });
    renderAt(<BodyPage />, '/body');
    await screen.findByText('80.8 kg');
    await userEvent.click(screen.getAllByRole('button', { name: 'Add entry' })[0]!);
    await userEvent.click(screen.getByRole('button', { name: 'Save entry' }));
    expect(screen.getByRole('alert')).toHaveTextContent('Fill in at least one');
    await userEvent.type(screen.getByLabelText('Weight (kg)'), '5');
    await userEvent.type(screen.getByLabelText('Waist (cm)'), 'abc');
    await userEvent.click(screen.getByRole('button', { name: 'Save entry' }));
    const weight = screen.getByLabelText('Weight (kg)');
    expect(weight).toHaveAttribute('aria-invalid', 'true');
    expect(weight).toHaveAccessibleDescription(/between 20 and 400 kg/);
    expect(screen.getByLabelText('Waist (cm)')).toHaveAccessibleDescription(/must be a number/);
    expect(calls.some((c) => c.init?.method === 'POST')).toBe(false);
  });

  it('posts the entry with the X-Nimrag header and reloads the list', async () => {
    let saved = false;
    const { calls } = mockApi({
      '/api/today': TODAY,
      '/api/body': () => json(saved ? body({ latest_weight: { value: 79.9, at: '2026-11-19T08:00:00+01:00' } }) : body()),
      'POST /api/body': () => {
        saved = true;
        return json({ id: 9, measured_at: '2026-11-19T08:00:00+01:00', weight_kg: 79.9, body_fat_pct: 18.5, waist_cm: null }, { status: 201 });
      },
    });
    renderAt(<BodyPage />, '/body');
    await screen.findByText('80.8 kg');
    await userEvent.click(screen.getAllByRole('button', { name: 'Add entry' })[0]!);
    await userEvent.type(screen.getByLabelText('Weight (kg)'), '79,9');
    await userEvent.type(screen.getByLabelText('Body fat (%)'), '18.5');
    await userEvent.click(screen.getByRole('button', { name: 'Save entry' }));
    const post = await waitFor(() => {
      const c = calls.find((x) => x.init?.method === 'POST');
      expect(c).toBeDefined();
      return c!;
    });
    expect(post.url).toBe('/api/body');
    expect((post.init!.headers as Record<string, string>)['X-Nimrag']).toBe('1');
    expect(JSON.parse(post.init!.body as string)).toEqual({ weight_kg: 79.9, body_fat_pct: 18.5 });
    expect(await screen.findByText('79.9 kg')).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Entry saved');
    expect(screen.queryByLabelText('Weight (kg)')).not.toBeInTheDocument();
  });

  it('shows the server error and keeps the form when saving fails', async () => {
    mockApi({
      '/api/today': TODAY,
      '/api/body': body(),
      'POST /api/body': () => json({ error: 'weight_kg must be between 20 and 400' }, { status: 400 }),
    });
    renderAt(<BodyPage />, '/body');
    await screen.findByText('80.8 kg');
    await userEvent.click(screen.getAllByRole('button', { name: 'Add entry' })[0]!);
    await userEvent.type(screen.getByLabelText('Weight (kg)'), '80');
    await userEvent.click(screen.getByRole('button', { name: 'Save entry' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('The entry was not saved. weight_kg must be between 20 and 400');
    expect(screen.getByLabelText('Weight (kg)')).toHaveValue('80');
  });

  it('asks before deleting, and only then sends DELETE with the X-Nimrag header', async () => {
    let gone = false;
    const { calls } = mockApi({
      '/api/today': TODAY,
      '/api/body': () => json(gone ? body({ entries: body().entries.slice(1) }) : body()),
      'DELETE /api/body/3': () => {
        gone = true;
        return new Response(null, { status: 204 });
      },
    });
    renderAt(<BodyPage />, '/body');
    await screen.findByText('80.8 kg');
    await userEvent.click(screen.getByRole('button', { name: 'Delete entry of Tue 17 Nov · 07:10' }));
    expect(calls.some((c) => c.init?.method === 'DELETE')).toBe(false);
    // changing one's mind sends nothing
    await userEvent.click(screen.getByRole('button', { name: 'Keep' }));
    expect(calls.some((c) => c.init?.method === 'DELETE')).toBe(false);
    await userEvent.click(screen.getByRole('button', { name: 'Delete entry of Tue 17 Nov · 07:10' }));
    await userEvent.click(screen.getByRole('button', { name: 'Delete for good' }));
    await waitFor(() => expect(calls.some((c) => c.init?.method === 'DELETE')).toBe(true));
    const del = calls.find((c) => c.init?.method === 'DELETE')!;
    expect(del.url).toBe('/api/body/3');
    expect((del.init!.headers as Record<string, string>)['X-Nimrag']).toBe('1');
    await waitFor(() => expect(screen.queryByText('Tue 17 Nov · 07:10')).not.toBeInTheDocument());
  });

  it('shows an error when the request fails', async () => {
    mockApi({ '/api/today': TODAY, '/api/body': () => json({ error: 'database busy' }, { status: 500 }) });
    renderAt(<BodyPage />, '/body');
    expect(await screen.findByRole('alert', {}, { timeout: 4000 })).toHaveTextContent('database busy');
  });
});

describe('CalendarPage (3l)', () => {
  const route = 'calendar/:month?';

  it('draws the month with a circle for cardio, a square for strength, and words for each day', async () => {
    const { calls } = mockApi({ '/api/today': TODAY, '/api/calendar/2026-11': calendar() });
    const { container } = renderAt(<CalendarPage />, '/calendar/2026-11', route);
    expect(await screen.findByRole('link', { name: '3 November, strength' })).toBeInTheDocument();
    expect(calls.some((c) => c.url === '/api/calendar/2026-11')).toBe(true);
    expect(screen.getByRole('link', { name: '5 November, run' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '10 November, run, strength' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '19 November, today' })).toHaveAttribute('aria-current', 'date');
    expect(container.querySelectorAll('.cal-day .dot--cardio')).toHaveLength(2);
    expect(container.querySelectorAll('.cal-day .dot--strength')).toHaveLength(3);
    expect(screen.getByText('Cardio (circle)')).toBeInTheDocument();
    expect(screen.getByText('Strength (square)')).toBeInTheDocument();
  });

  it('shows month totals', async () => {
    mockApi({ '/api/today': TODAY, '/api/calendar/2026-11': calendar() });
    renderAt(<CalendarPage />, '/calendar/2026-11', route);
    const totals = (await screen.findByRole('heading', { name: 'Month so far' })).closest('section') as HTMLElement;
    expect(within(totals).getByText('Activities').nextElementSibling).toHaveTextContent('5');
    expect(within(totals).getByText('Strength sessions').nextElementSibling).toHaveTextContent('3');
    expect(within(totals).getByText('Cardio sessions').nextElementSibling).toHaveTextContent('2');
  });

  it('taps a day to open /day/{date}', async () => {
    mockApi({ '/api/today': TODAY, '/api/calendar/2026-11': calendar() });
    renderAt(<CalendarPage />, '/calendar/2026-11', route);
    const day = await screen.findByRole('link', { name: '3 November, strength' });
    expect(day).toHaveAttribute('href', '/day/2026-11-03');
    expect(screen.getByRole('link', { name: '1 November' })).toHaveAttribute('href', '/day/2026-11-01');
  });

  it('previous and next month are in the URL', async () => {
    mockApi({ '/api/today': TODAY, '/api/calendar/': calendar() });
    renderAt(<CalendarPage />, '/calendar/2026-11', route);
    await screen.findByRole('link', { name: '3 November, strength' });
    expect(screen.getByRole('link', { name: 'Previous month' })).toHaveAttribute('href', '/calendar/2026-10');
    expect(screen.getByRole('link', { name: 'Next month' })).toHaveAttribute('href', '/calendar/2026-12');
  });

  it('opens the current month when the URL has none', async () => {
    const { calls } = mockApi({ '/api/today': TODAY, '/api/calendar/2026-11': calendar() });
    renderAt(<CalendarPage />, '/calendar', route);
    await screen.findByRole('link', { name: '3 November, strength' });
    expect(calls.some((c) => c.url === '/api/calendar/2026-11')).toBe(true);
    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('November 2026');
  });

  it('is one tab stop and moves with the arrow keys', async () => {
    mockApi({ '/api/today': TODAY, '/api/calendar/2026-11': calendar() });
    renderAt(<CalendarPage />, '/calendar/2026-11', route);
    const today = await screen.findByRole('link', { name: '19 November, today' });
    const stops = screen.getAllByRole('link').filter((l) => l.getAttribute('tabindex') === '0' && l.hasAttribute('data-date'));
    expect(stops).toEqual([today]);
    today.focus();
    await userEvent.keyboard('{ArrowRight}');
    expect(screen.getByRole('link', { name: '20 November' })).toHaveFocus();
    await userEvent.keyboard('{ArrowDown}');
    expect(screen.getByRole('link', { name: '27 November' })).toHaveFocus();
    await userEvent.keyboard('{ArrowLeft}');
    expect(screen.getByRole('link', { name: '26 November' })).toHaveFocus();
    await userEvent.keyboard('{ArrowUp}');
    expect(screen.getByRole('link', { name: '19 November, today' })).toHaveFocus();
    await userEvent.keyboard('{Home}');
    expect(screen.getByRole('link', { name: '16 November' })).toHaveFocus();
    await userEvent.keyboard('{End}');
    expect(screen.getByRole('link', { name: '22 November' })).toHaveFocus();
    // the focused day is the only tab stop
    const after = screen.getAllByRole('link').filter((l) => l.getAttribute('tabindex') === '0' && l.hasAttribute('data-date'));
    expect(after).toHaveLength(1);
    expect(after[0]).toHaveFocus();
  });

  it('crossing the end of the month with an arrow opens the next month', async () => {
    mockApi({
      '/api/today': TODAY,
      '/api/calendar/2026-11': calendar(),
      '/api/calendar/2026-12': { ...calendar(), month: '2026-12', days: calendar().days.map((d) => ({ ...d, date: d.date.replace('2026-11', '2026-12') })) },
    });
    renderAt(<CalendarPage />, '/calendar/2026-11', route);
    const last = await screen.findByRole('link', { name: '30 November' });
    last.focus();
    await userEvent.keyboard('{ArrowRight}');
    await waitFor(() => expect(screen.getByTestId('where')).toHaveTextContent('/calendar/2026-12'));
    expect(await screen.findByRole('link', { name: '1 December' })).toHaveFocus();
  });

  it('the focused day shows its activities, sleep, steps, heart rate and energy', async () => {
    mockApi({ '/api/today': TODAY, '/api/calendar/2026-11': calendar() });
    renderAt(<CalendarPage />, '/calendar/2026-11', route);
    const day = await screen.findByRole('link', { name: '10 November, run, strength' });
    fireEvent.focus(day);
    const card = screen.getByRole('heading', { name: 'Tue 10 Nov' }).closest('section') as HTMLElement;
    expect(within(card).getByText('Run · 18:05')).toBeInTheDocument();
    expect(within(card).getByText('Strength · 18:05')).toBeInTheDocument();
    expect(within(card).getByText('42m · 18 sets')).toBeInTheDocument();
    expect(within(card).getByText('7h 12m · score 81')).toBeInTheDocument();
    expect(within(card).getByText('8,360')).toBeInTheDocument();
    expect(within(card).getByText('51 bpm')).toBeInTheDocument();
    expect(within(card).getByText('High 94 · low 38')).toBeInTheDocument();
    expect(within(card).getByRole('link', { name: 'Open day view' })).toHaveAttribute('href', '/day/2026-11-10');
  });

  it('a day without data says so: no activity, unavailable metrics', async () => {
    mockApi({ '/api/today': TODAY, '/api/calendar/2026-11': calendar() });
    renderAt(<CalendarPage />, '/calendar/2026-11', route);
    const day = await screen.findByRole('link', { name: '1 November' });
    fireEvent.focus(day);
    const card = screen.getByRole('heading', { name: 'Sun 1 Nov' }).closest('section') as HTMLElement;
    expect(within(card).getByText('No activity on this day.')).toBeInTheDocument();
    expect(within(card).getAllByText('unavailable')).toHaveLength(4);
  });

  it('shows an error state with a retry', async () => {
    mockApi({ '/api/today': TODAY, '/api/calendar/': () => json({ error: 'boom' }, { status: 500 }) });
    renderAt(<CalendarPage />, '/calendar/2026-11', route);
    expect(await screen.findByRole('alert', {}, { timeout: 4000 })).toHaveTextContent('boom');
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
  });
});
