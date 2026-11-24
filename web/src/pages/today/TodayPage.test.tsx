import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { HomeCards } from '../../api/generated';
import { TodayPage } from '../TodayPage';
import { makeEmptyToday, makeSettings, makeToday } from './testData';

// uPlot draws on a canvas, which jsdom does not have: swap the chart for its accessible text.
vi.mock('../../components/SeriesChart', async () => {
  const React = await import('react');
  return {
    CHART_COLORS: {},
    SeriesChart: ({ label, summary }: { label: string; summary: string }) =>
      React.createElement('div', { role: 'img', 'aria-label': label }, React.createElement('p', null, summary)),
  };
});

type Handler = (init: RequestInit | undefined) => Response | Promise<Response>;

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
}

const ACTIVITIES = {
  sample: true,
  next_before: null,
  items: [
    { id: 'a1', kind: 'run', sport_name: 'running', sub_sport_name: null, start: '2026-11-19T08:00:00+00:00', date: '2026-11-19', duration_s: 2220, distance_m: 6400, avg_pace_s_per_km: 345, avg_hr_bpm: 152, max_hr_bpm: 170, calories_kcal: 400, ascent_m: 20, sets: null, volume_kg: null, exercises: null },
    { id: 'a2', kind: 'strength', sport_name: 'training', sub_sport_name: 'strength_training', start: '2026-11-18T08:00:00+00:00', date: '2026-11-18', duration_s: 2700, distance_m: null, avg_pace_s_per_km: null, avg_hr_bpm: null, max_hr_bpm: null, calories_kcal: null, ascent_m: null, sets: 18, volume_kg: 3600, exercises: 6 },
  ],
};

const desktopRoutes: Record<string, Handler> = {
  '/api/activities?limit=100': () => json(ACTIVITIES),
  '/api/sleep?nights=14': () =>
    json({ sample: true, average_duration_s: 25000, average_score: 72, nights: [{ date: '2026-11-17', start: '', end: '', duration_s: 24000, score: 70, deep_s: null, light_s: null, rem_s: null, awake_s: null }, { date: '2026-11-18', start: '', end: '', duration_s: 27000, score: 80, deep_s: null, light_s: null, rem_s: null, awake_s: null }] }),
  '/api/storage': () => json({ sample: true, files: 1284, bytes: 222298112, by_source: [], parsed_ok: 1284, parsed_partial: 0, parsed_error: 0, database_bytes: 5865528, last_backup_at: null }),
  '/api/devices': () => json({ sample: true, watch: { model: null, firmware: null, battery_percent: 74, battery_at: null, last_data_at: '2026-11-19T07:40:00+00:00' }, last_sync_at: '2026-11-19T07:42:00+00:00', new_files_24h: 5, upload_devices: [] }),
  '/api/workouts': () => json({ watch_send: { available: false }, limits: {}, workouts: [{ id: 7, name: 'Tempo 5k', sport: 'running', steps: 4, duration_s: 1800, updated_at: '2026-11-18T10:00:00+00:00', send: { state: 'idle', requested_at: null, finished_at: null, error: null } }] }),
};

let calls: { url: string; init: RequestInit | undefined }[] = [];

function mockApi(routes: Record<string, Handler>) {
  calls = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, init });
      const handler = routes[url];
      if (!handler) return json({ error: `no route for ${url}` }, 404);
      return handler(init);
    }),
  );
}

function setDesktop(on: boolean) {
  window.matchMedia = ((query: string) => ({
    matches: on && query.includes('min-width: 1024px'),
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
}

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } });
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <TodayPage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return { client, ...view };
}

const glanceIds = (root: ParentNode = document) => Array.from(root.querySelectorAll<HTMLElement>('.glance-grid [data-card]')).map((el) => el.dataset['card']);
const cardIds = (selector: string) => Array.from(document.querySelectorAll<HTMLElement>(`${selector} [data-card]`)).map((el) => el.dataset['card']);

beforeEach(() => setDesktop(false));
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.style.overflow = '';
});

describe('Today on a phone', () => {
  it('renders the structure from the API: In focus, At a glance, Yesterday, Last 7 days, Edit home', async () => {
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(makeSettings()) });
    renderPage();

    expect(await screen.findByRole('heading', { level: 1, name: 'Today' })).toBeInTheDocument();
    expect(await screen.findByRole('heading', { name: 'In focus' })).toBeInTheDocument();
    expect(screen.getByText('Thursday 19 November')).toBeInTheDocument();
    expect(screen.getByText('Sample data')).toBeInTheDocument();
    expect(screen.getByText('Watch synced 07:42')).toBeInTheDocument();
    expect(screen.getByText('+54 charged')).toBeInTheDocument();
    expect(screen.getByText('+54 charged').nextSibling).toHaveTextContent('−31 drained');
    expect(screen.getByRole('heading', { name: 'At a glance' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Yesterday' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Last 7 days' })).toBeInTheDocument();
    expect(screen.getByText('4 · 2 run, 2 strength')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Edit home' })).toBeEnabled();
    // Our word is Energy.
    expect(document.body.textContent).not.toMatch(/body battery/i);
  });

  it('shows no metric twice: Energy, Sleep and HRV are in In focus and left out of At a glance', async () => {
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(makeSettings({ home_cards: { order: makeSettings().home_cards.order, hidden: [] } })) });
    renderPage();
    await screen.findByRole('heading', { name: 'At a glance' });
    await waitFor(() => expect(glanceIds().length).toBeGreaterThan(0));
    for (const id of ['energy', 'sleep', 'hrv']) expect(glanceIds()).not.toContain(id);
    // The one Energy figure is the focus slide's.
    expect(screen.getAllByText('Energy')).toHaveLength(1);
  });

  it('writes the training status date like the rest of the app, not as an ISO date', async () => {
    const status = { ...makeToday().training_status, label: 'Maintaining', kind: 'maintaining' as const };
    mockApi({ '/api/today': () => json(makeToday({ training_status: status })), '/api/settings': () => json(makeSettings({ home_cards: { order: makeSettings().home_cards.order, hidden: [] } })) });
    renderPage();
    const card = await waitFor(() => {
      const el = document.querySelector('.glance-grid [data-card="training_status"]');
      if (!el) throw new Error('no card');
      return el as HTMLElement;
    });
    expect(card).toHaveTextContent('Since Sun 1 Nov');
    expect(document.body.textContent).not.toMatch(/\d{4}-\d{2}-\d{2}/);
  });

  it('says Recovered, not "0m, ready around now", when no recovery time is left', async () => {
    const base = makeToday();
    const recovery_time = { ...base.recovery_time, remaining_s: 0, percent_complete: 100, ready_at: '2026-11-21T08:54:00+00:00' };
    mockApi({ '/api/today': () => json(makeToday({ recovery_time })), '/api/settings': () => json(makeSettings({ home_cards: { order: makeSettings().home_cards.order, hidden: [] } })) });
    renderPage();
    const card = (await screen.findByText('Recovery time')).closest('a, .stat') as HTMLElement;
    expect(within(card).getByText('Recovered')).toBeInTheDocument();
    expect(within(card).queryByText(/Ready around/)).toBeNull();
  });

  it('keeps the Yesterday and Last 7 days labels short enough for large text', async () => {
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(makeSettings()) });
    renderPage();
    const yesterday = await screen.findByRole('heading', { name: 'Yesterday' });
    const labels = Array.from(yesterday.closest('section')!.querySelectorAll('dt')).map((dt) => dt.textContent ?? '');
    expect(labels.length).toBeGreaterThan(0);
    for (const l of labels) expect(l.length).toBeLessThanOrEqual(16);
    const last7 = screen.getByRole('heading', { name: 'Last 7 days' });
    for (const dt of Array.from(last7.closest('section')!.querySelectorAll('dt'))) expect((dt.textContent ?? '').length).toBeLessThanOrEqual(16);
  });

  it('says the days follow UTC, with a link to the timezone page, when the account is on UTC and the browser is not', async () => {
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({ timeZone: 'Europe/Copenhagen' } as Intl.ResolvedDateTimeFormatOptions);
    mockApi({ '/api/today': () => json(makeToday({ timezone: 'Etc/UTC' })), '/api/settings': () => json(makeSettings()) });
    renderPage();
    expect(await screen.findByText(/Your days follow UTC time\./)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Set your timezone' })).toHaveAttribute('href', '/profile/timezone');
  });

  it('shows no timezone notice when the browser is on UTC too, or the account is not', async () => {
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({ timeZone: 'UTC' } as Intl.ResolvedDateTimeFormatOptions);
    mockApi({ '/api/today': () => json(makeToday({ timezone: 'Etc/UTC' })), '/api/settings': () => json(makeSettings()) });
    renderPage();
    await screen.findByRole('heading', { name: 'In focus' });
    expect(screen.queryByText(/follow UTC time/)).not.toBeInTheDocument();
  });

  it('shows the cards in the order of home_cards and leaves hidden ones out', async () => {
    const settings = makeSettings({
      home_cards: { order: ['steps', 'sleep', 'weight', 'energy', 'heart_rate', 'stress', 'hrv', 'training_status', 'intensity_minutes', 'training_load', 'recovery_time', 'vo2_max', 'respiration', 'spo2', 'training_readiness'], hidden: ['sleep', 'stress', 'respiration', 'spo2', 'training_readiness'] },
    });
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(settings) });
    renderPage();
    await screen.findByRole('heading', { name: 'At a glance' });
    // Energy sits in In focus, so At a glance skips it.
    await waitFor(() => expect(glanceIds().slice(0, 3)).toEqual(['steps', 'weight', 'heart_rate']));
    expect(glanceIds()).not.toContain('sleep');
    expect(glanceIds()).not.toContain('stress');
  });

  it('each card links to its detail page', async () => {
    const all: HomeCards = { order: makeSettings().home_cards.order, hidden: [] };
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(makeSettings({ home_cards: all })) });
    renderPage();
    await screen.findByRole('heading', { name: 'At a glance' });
    await waitFor(() => expect(glanceIds()).toHaveLength(12));
    const href = (id: string) => document.querySelector(`.glance-grid [data-card="${id}"]`)?.getAttribute('href');
    expect(href('heart_rate')).toBe('/metrics/heart_rate');
    expect(href('stress')).toBe('/metrics/stress');
    expect(href('steps')).toBe('/metrics/steps');
    expect(href('training_load')).toBe('/metrics/training_load');
    expect(href('intensity_minutes')).toBe('/metrics/intensity_minutes');
    expect(href('vo2_max')).toBe('/metrics/vo2_max');
    expect(href('recovery_time')).toBe('/training');
    expect(href('weight')).toBe('/body');
  });

  it('missing metrics read "unavailable", never 0', async () => {
    mockApi({ '/api/today': () => json(makeEmptyToday()), '/api/settings': () => json(makeSettings({ home_cards: { order: makeSettings().home_cards.order, hidden: [] } })) });
    renderPage();
    await screen.findByRole('heading', { name: 'At a glance' });
    await waitFor(() => expect(glanceIds()).toHaveLength(12));
    expect(screen.getAllByText('unavailable').length).toBeGreaterThan(10);
    expect(screen.queryByText(/^0$/)).not.toBeInTheDocument();
    expect(screen.getByText('No watch data yet')).toBeInTheDocument();
    // The sleep slide says so instead of showing a zero night.
    expect(screen.getByText('The watch recorded no sleep for last night.')).toBeInTheDocument();
    const steps = document.querySelector('.glance-grid [data-card="steps"]') as HTMLElement;
    expect(within(steps).getByText('unavailable')).toBeInTheDocument();
    expect(within(steps).getByRole('progressbar', { name: 'unavailable of 8,000 steps' })).toBeInTheDocument();
  });

  it('shows a loading state, then an error with "Try again"', async () => {
    mockApi({ '/api/today': () => json({ error: 'database busy' }, 500), '/api/settings': () => json(makeSettings()) });
    renderPage();
    expect(screen.getByLabelText('Loading today')).toHaveAttribute('aria-busy', 'true');
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not load today');
    expect(screen.getByRole('alert')).toHaveTextContent('database busy');
    const before = calls.filter((c) => c.url === '/api/today').length;
    await userEvent.click(screen.getByRole('button', { name: 'Try again' }));
    await waitFor(() => expect(calls.filter((c) => c.url === '/api/today').length).toBe(before + 1));
  });

  it('falls back to the default card order when settings cannot be loaded', async () => {
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json({ error: 'nope' }, 500) });
    renderPage();
    await screen.findByText(/saved card layout could not be loaded/);
    expect(glanceIds()[0]).toBe('heart_rate');
    expect(glanceIds()).not.toContain('weight');
    expect(screen.getByRole('button', { name: 'Edit home' })).toBeDisabled();
  });
});

describe('In focus carousel', () => {
  it('has labelled dots as buttons and moves with the arrow keys', async () => {
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(makeSettings()) });
    renderPage();
    const region = await screen.findByRole('region', { name: 'In focus' });
    const dots = within(region).getAllByRole('button');
    expect(dots.map((d) => d.getAttribute('aria-label'))).toEqual(['Show Energy, card 1 of 3', 'Show Sleep, card 2 of 3', 'Show HRV, card 3 of 3']);
    expect(dots[0]).toHaveAttribute('aria-current', 'true');

    const track = within(region).getByRole('group', { name: /In focus cards/ });
    expect(track).toHaveAttribute('tabindex', '0');
    track.focus();
    await userEvent.keyboard('{ArrowRight}');
    expect(dots[1]).toHaveAttribute('aria-current', 'true');
    await userEvent.keyboard('{ArrowRight}{ArrowRight}');
    expect(dots[2]).toHaveAttribute('aria-current', 'true');
    await userEvent.keyboard('{ArrowLeft}');
    expect(dots[1]).toHaveAttribute('aria-current', 'true');
    await userEvent.click(dots[0]!);
    expect(dots[0]).toHaveAttribute('aria-current', 'true');
  });

  it('puts Energy, Sleep last night and HRV in the three slides', async () => {
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(makeSettings()) });
    renderPage();
    const region = await screen.findByRole('region', { name: 'In focus' });
    expect(within(region).getByRole('img', { name: 'Energy level 68 out of 100' })).toBeInTheDocument();
    expect(within(region).getByText('Good reserve')).toBeInTheDocument();
    expect(within(region).getByText(/Woke 06:36/)).toBeInTheDocument();
    expect(within(region).getByText('7h 12m')).toBeInTheDocument();
    expect(within(region).getByText('Score 81, good')).toBeInTheDocument();
    expect(within(region).getByRole('img', { name: /^Sleep stages: deep 1h 40m/ })).toBeInTheDocument();
    expect(within(region).getByText('62 ms')).toBeInTheDocument();
    expect(within(region).getByText('Within your range')).toBeInTheDocument();
  });
});

describe('Edit home', () => {
  const open = async () => {
    const user = userEvent.setup();
    await screen.findByRole('heading', { name: 'At a glance' });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Edit home' })).toBeEnabled());
    const opener = screen.getByRole('button', { name: 'Edit home' });
    await user.click(opener);
    const dialog = await screen.findByRole('dialog', { name: 'Edit home' });
    return { user, opener, dialog };
  };

  it('opens a modal dialog, traps Tab, closes on Escape and returns focus', async () => {
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(makeSettings()) });
    renderPage();
    const { user, opener, dialog } = await open();
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveFocus();

    screen.getByRole('button', { name: 'Save' }).focus();
    await user.tab();
    expect(within(dialog).getByRole('checkbox', { name: 'Energy' })).toHaveFocus();
    await user.tab({ shift: true });
    expect(screen.getByRole('button', { name: 'Save' })).toHaveFocus();

    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it('lists every card with show/hide toggles reflecting the saved layout', async () => {
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(makeSettings()) });
    renderPage();
    const { dialog } = await open();
    expect(within(dialog).getAllByRole('checkbox')).toHaveLength(15);
    expect(within(dialog).getByRole('checkbox', { name: 'Steps' })).toBeChecked();
    expect(within(dialog).getByRole('checkbox', { name: /Body weight/ })).not.toBeChecked();
    expect(dialog.querySelectorAll('.edit-row__state')).toHaveLength(4);
  });

  it('reorders with the move buttons and keeps keyboard focus on the moved row', async () => {
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(makeSettings()) });
    renderPage();
    const { user, dialog } = await open();
    const names = () => within(dialog).getAllByRole('checkbox').map((c) => c.closest('label')?.textContent?.replace(' (hidden)', ''));
    expect(names().slice(0, 3)).toEqual(['Energy', 'Heart rate', 'Stress']);
    expect(within(dialog).getByRole('button', { name: 'Move Energy up' })).toBeDisabled();

    await user.click(within(dialog).getByRole('button', { name: 'Move Heart rate up' }));
    expect(names().slice(0, 3)).toEqual(['Heart rate', 'Energy', 'Stress']);
    // Heart rate is first now: its up button is disabled, so focus lands on its down button.
    expect(within(dialog).getByRole('button', { name: 'Move Heart rate down' })).toHaveFocus();
    expect(screen.getByRole('status')).toHaveTextContent('Heart rate moved to position 1 of 15');

    await user.keyboard('{Enter}');
    expect(names().slice(0, 3)).toEqual(['Energy', 'Heart rate', 'Stress']);
  });

  it('reorders by drag and drop', async () => {
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(makeSettings()) });
    renderPage();
    const { dialog } = await open();
    const row = (id: string) => dialog.querySelector(`[data-card="${id}"]`) as HTMLElement;
    const dataTransfer = { setData: () => {}, effectAllowed: '', dropEffect: '' };
    fireEvent.dragStart(row('steps'), { dataTransfer });
    fireEvent.dragOver(row('energy'), { dataTransfer });
    fireEvent.drop(row('energy'), { dataTransfer });
    fireEvent.dragEnd(row('steps'), { dataTransfer });
    const order = Array.from(dialog.querySelectorAll<HTMLElement>('[data-card]')).map((el) => el.dataset['card']);
    expect(order.slice(0, 3)).toEqual(['steps', 'energy', 'heart_rate']);
  });

  it('saves with PATCH and the X-Nimrag header, and updates the cards before the server answers', async () => {
    let finish: (r: Response) => void = () => {};
    const pending = new Promise<Response>((resolve) => {
      finish = resolve;
    });
    mockApi({
      '/api/today': () => json(makeToday()),
      '/api/settings': (init) => (init?.method === 'PATCH' ? pending : json(makeSettings())),
    });
    renderPage();
    const { user, dialog } = await open();
    await user.click(within(dialog).getByRole('checkbox', { name: 'Steps' }));
    expect(within(dialog).getByRole('checkbox', { name: /Steps/ })).not.toBeChecked();
    await user.click(within(dialog).getByRole('button', { name: 'Move Heart rate up' }));
    await user.click(screen.getByRole('button', { name: 'Save' }));

    // The dialog is gone and the grid already follows the new layout while the request is pending.
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    await waitFor(() => expect(glanceIds().slice(0, 2)).toEqual(['heart_rate', 'stress']));
    expect(glanceIds()).not.toContain('steps');

    const patch = calls.find((c) => c.init?.method === 'PATCH');
    expect(patch?.url).toBe('/api/settings');
    expect((patch?.init?.headers as Record<string, string>)['X-Nimrag']).toBe('1');
    const body = JSON.parse(String(patch?.init?.body)) as { home_cards: HomeCards };
    expect(body.home_cards.order.slice(0, 2)).toEqual(['heart_rate', 'energy']);
    expect(body.home_cards.hidden).toContain('steps');
    expect(body.home_cards.hidden).toContain('weight');

    finish(json(makeSettings({ home_cards: body.home_cards })));
    await waitFor(() => expect(glanceIds()).not.toContain('steps'));
  });

  it('rolls back and explains when saving fails', async () => {
    mockApi({
      '/api/today': () => json(makeToday()),
      '/api/settings': (init) => (init?.method === 'PATCH' ? json({ error: 'order lists steps twice' }, 400) : json(makeSettings())),
    });
    renderPage();
    const { user, dialog } = await open();
    await user.click(within(dialog).getByRole('checkbox', { name: 'Steps' }));
    await user.click(screen.getByRole('button', { name: 'Save' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Could not save your layout: order lists steps twice');
    expect(alert).toHaveTextContent('previous layout is back');
    // The sheet is open again with the unsaved change, and the page behind shows the old layout.
    const dialog2 = screen.getByRole('dialog', { name: 'Edit home' });
    expect(within(dialog2).getByRole('checkbox', { name: /Steps/ })).not.toBeChecked();
    expect(glanceIds()).toContain('steps');
  });

  it('reset to default restores the default order and hidden cards', async () => {
    const custom = makeSettings({ home_cards: { order: [...makeSettings().home_cards.order].reverse(), hidden: ['energy'] } });
    mockApi({ '/api/today': () => json(makeToday()), '/api/settings': () => json(custom) });
    renderPage();
    const { user, dialog } = await open();
    expect(within(dialog).getByRole('checkbox', { name: /Energy/ })).not.toBeChecked();
    await user.click(within(dialog).getByRole('button', { name: 'Reset to default' }));
    const order = Array.from(dialog.querySelectorAll<HTMLElement>('[data-card]')).map((el) => el.dataset['card']);
    expect(order).toEqual(makeSettings().home_cards.order);
    expect(within(dialog).getByRole('checkbox', { name: /Energy/ })).toBeChecked();
    expect(within(dialog).getByRole('checkbox', { name: /Body weight/ })).not.toBeChecked();
  });
});

describe('Home on a desktop', () => {
  const desktopApi = (extra: Record<string, Handler> = {}, today = makeToday(), settings = makeSettings()) => {
    setDesktop(true);
    mockApi({ '/api/today': () => json(today), '/api/settings': () => json(settings), ...desktopRoutes, ...extra });
  };

  it('renders In Focus, At a Glance, Yesterday, Last 7 Days and the right column', async () => {
    desktopApi();
    renderPage();

    expect(await screen.findByRole('heading', { level: 1, name: 'Home' })).toBeInTheDocument();
    expect(await screen.findByText('Watch synced 07:42')).toBeInTheDocument();
    expect(screen.getByText('Sample data')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'In Focus' })).toBeInTheDocument();
    await waitFor(() => expect(cardIds('.hgrid--focus')).toEqual(['energy', 'week_activities', 'week_running', 'steps', 'sleep', 'training_status']));
    // Heading, then the link carries the name.
    const energy = document.querySelector<HTMLElement>('[data-card="energy"]')!;
    expect(within(energy).getByRole('link', { name: 'Energy' })).toHaveAttribute('href', '/metrics/energy');
    expect(energy).toHaveTextContent('Charged');

    expect(screen.getByRole('heading', { name: 'At a Glance' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'See All' })).toBeInTheDocument();
    // In Focus already shows Energy, Steps, Sleep and Training status; HRV is a glance card here.
    expect(cardIds('.hgrid--glance')).toEqual(['heart_rate', 'stress', 'hrv', 'intensity_minutes', 'training_load', 'recovery_time', 'vo2_max']);

    expect(screen.getByRole('heading', { name: 'Yesterday' })).toBeInTheDocument();
    const yesterday = screen.getByRole('region', { name: 'Yesterday' });
    expect(within(yesterday).getByRole('link', { name: 'Steps' })).toHaveAttribute('href', '/metrics/steps');
    expect(yesterday).toHaveTextContent('114% of 8,000');
    expect(yesterday).toHaveTextContent('81 / 7h 12m');
    expect(yesterday).toHaveTextContent('51 Rest / 148 High');
    const week = screen.getByRole('region', { name: 'Last 7 Days' });
    expect(week).toHaveTextContent('4 · 2 run, 2 strength');
    expect(week).toHaveTextContent('8,441 Avg');

    expect(screen.getByRole('button', { name: 'Edit Home' })).toBeEnabled();
    // Our word is Energy.
    expect(document.body.textContent).not.toMatch(/body battery/i);
  });

  it('the right column lists recent activities, sync and watch, and workouts', async () => {
    desktopApi();
    renderPage();

    const list = await screen.findByRole('list', { name: 'Recent activities' });
    const items = within(list).getAllByRole('listitem');
    expect(items).toHaveLength(2);
    expect(within(items[0]!).getByRole('link', { name: 'Run' })).toHaveAttribute('href', '/activities/a1');
    expect(items[0]).toHaveTextContent('Thu 19 Nov');
    expect(items[0]).toHaveTextContent('6.40 km');
    expect(items[0]).toHaveTextContent('37:00');
    expect(items[1]).toHaveTextContent('18');
    expect(items[1]).toHaveTextContent('Sets');

    expect(await screen.findByText('1,284 original files · 212 MB')).toBeInTheDocument();
    expect(screen.getByText('Battery 74%')).toBeInTheDocument();
    // Backups are not built yet: the API sends null, so the row is left out instead of saying "unavailable".
    expect(screen.queryByText('Last backup')).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Open the Watch page' })).toHaveAttribute('href', '/devices');

    expect(await screen.findByRole('link', { name: 'Tempo 5k' })).toHaveAttribute('href', '/workouts/7');
    expect(screen.getByRole('link', { name: 'Open workouts' })).toHaveAttribute('href', '/workouts');
    expect(calls.map((c) => c.url)).toEqual(expect.arrayContaining(['/api/activities?limit=100', '/api/sleep?nights=14', '/api/storage', '/api/devices', '/api/workouts']));
  });

  it('the week cards total the last 7 days of activities', async () => {
    desktopApi();
    renderPage();
    const all = await waitFor(() => {
      const el = document.querySelector('[data-card="week_activities"]');
      if (!el || !el.textContent?.includes('1h 22m')) throw new Error('not ready');
      return el as HTMLElement;
    });
    expect(all).toHaveTextContent('All activities · Nov 13–Nov 19');
    expect(all).toHaveTextContent('2Activities');
    const run = document.querySelector('[data-card="week_running"]') as HTMLElement;
    expect(run).toHaveTextContent('6.4 km');
    expect(run).toHaveTextContent('5:47 /km');
    expect(within(run).getByRole('img', { name: /Running per day/ })).toBeInTheDocument();
    expect(within(run).getByRole('img', { name: /1 day with a run in the last 4 weeks/ })).toBeInTheDocument();
  });

  it('leaves out the focus cards hidden in Edit Home, and follows the order for At a Glance', async () => {
    desktopApi({}, makeToday(), makeSettings({ home_cards: { order: ['vo2_max', 'energy', 'heart_rate', 'steps', 'sleep', 'hrv', 'stress', 'training_status', 'intensity_minutes', 'training_load', 'recovery_time', 'respiration', 'spo2', 'weight', 'training_readiness'], hidden: ['energy', 'stress', 'respiration', 'spo2', 'weight', 'training_readiness'] } }));
    renderPage();
    await screen.findByRole('heading', { name: 'At a Glance' });
    await waitFor(() => expect(cardIds('.hgrid--glance')).toEqual(['vo2_max', 'heart_rate', 'hrv', 'intensity_minutes', 'training_load', 'recovery_time']));
    expect(cardIds('.hgrid--focus')).toEqual(['week_activities', 'week_running', 'steps', 'sleep', 'training_status']);
  });

  it('shows calm empty states, never zeros, when the watch gave no data', async () => {
    desktopApi({ '/api/activities?limit=100': () => json({ sample: true, items: [], next_before: null }) }, makeEmptyToday(), makeSettings({ home_cards: { order: makeSettings().home_cards.order, hidden: ['respiration', 'spo2', 'weight'] } }));
    renderPage();
    expect(await screen.findByText('No energy data yet today.')).toBeInTheDocument();
    expect(screen.getByText('No sleep detected last night.')).toBeInTheDocument();
    expect(screen.getByText('No steps recorded yet today.')).toBeInTheDocument();
    expect(screen.getByText('No training status yet.')).toBeInTheDocument();
    expect(screen.getByText('Wear your watch while sleeping to reveal your status.')).toBeInTheDocument();
    expect(await screen.findByText('No runs this week.')).toBeInTheDocument();
    expect(screen.getByText('No activities in the last 7 days.')).toBeInTheDocument();
    expect(screen.getByText('Not provided by this watch yet.')).toBeInTheDocument();
    expect(screen.getByText('The watch has no data for this day.')).toBeInTheDocument();
    expect(await screen.findByText(/No activities yet/)).toBeInTheDocument();
  });

  it('opens Edit Home with the desktop hint and saves a new layout', async () => {
    desktopApi({ '/api/settings': (init) => (init?.method === 'PATCH' ? json(makeSettings()) : json(makeSettings())) });
    renderPage();
    await userEvent.click(await screen.findByRole('button', { name: 'Edit Home' }));
    const dialog = await screen.findByRole('dialog', { name: 'Edit home' });
    expect(dialog).toHaveTextContent('shown under In Focus');
    await userEvent.click(within(dialog).getByRole('checkbox', { name: /Heart rate/ }));
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(calls.some((c) => c.init?.method === 'PATCH')).toBe(true));
    const patch = calls.find((c) => c.init?.method === 'PATCH')!;
    expect(JSON.parse(String(patch.init?.body)).home_cards.hidden).toContain('heart_rate');
  });

  it('shows an error without losing the rest of the page when activities fail', async () => {
    desktopApi({ '/api/activities?limit=100': () => json({ error: 'boom' }, 500) });
    renderPage();
    expect(await screen.findByText('Could not load recent activities.')).toBeInTheDocument();
    expect(document.querySelector('[data-card="energy"]')).toBeInTheDocument();
    expect(screen.getAllByText('Could not load your activities.')).toHaveLength(2);
  });
});
