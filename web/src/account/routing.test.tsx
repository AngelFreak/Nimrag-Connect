import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { json, mockApi } from '../pages/body/testUtils';

const me = (over: Record<string, unknown> = {}) => ({ authenticated: true, username: 'demo', role: 'user', timezone: 'Europe/Copenhagen', sample: false, ...over });
const adminMe = me({ username: 'boss', role: 'admin' });

function Where() {
  const l = useLocation();
  return <div data-testid="where">{l.pathname + l.search}</div>;
}

function renderApp(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <App />
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const adminApi = (over: Record<string, unknown> = {}) => ({
  '/api/me': adminMe,
  '/api/admin/users': { users: [] },
  '/api/admin/unassigned': { tokens: [], unattributed_files: 0, legacy: { settings: false, body_entries: 0, strength_edits: 0, garmin_account: false } },
  ...over,
});

const NOT_SIGNED_IN = () => json({ error: 'not logged in' }, { status: 401 });

afterEach(() => vi.unstubAllGlobals());

afterEach(() => vi.unstubAllGlobals());

describe('who sees what', () => {
  it('nobody signed in: any data page leads to the login page, remembering where', async () => {
    mockApi({ '/api/me': NOT_SIGNED_IN, '/api/setup': { needed: false } });
    renderApp('/activities');
    expect(await screen.findByLabelText('Username')).toBeInTheDocument();
    expect(screen.getByTestId('where')).toHaveTextContent('/login?next=%2Factivities');
  });

  it('inside the Android app nobody signed in is handed to the app: no login page', async () => {
    const sessionExpired = vi.fn();
    window.NimragApp = { native: true, sessionExpired };
    try {
      mockApi({ '/api/me': NOT_SIGNED_IN, '/api/setup': { needed: false } });
      renderApp('/activities');
      await waitFor(() => expect(sessionExpired).toHaveBeenCalled());
      expect(screen.queryByLabelText('Username')).not.toBeInTheDocument();
      expect(screen.getByTestId('where')).toHaveTextContent('/activities');
    } finally {
      delete window.NimragApp;
    }
  });

  it('inside the Android app the login page itself is never shown', async () => {
    const sessionExpired = vi.fn();
    window.NimragApp = { native: true, sessionExpired };
    try {
      mockApi({ '/api/me': NOT_SIGNED_IN, '/api/setup': { needed: false } });
      renderApp('/login');
      await waitFor(() => expect(sessionExpired).toHaveBeenCalled());
      expect(screen.queryByLabelText('Username')).not.toBeInTheDocument();
    } finally {
      delete window.NimragApp;
    }
  });

  it('nobody signed in: /admin leads to the login page too', async () => {
    mockApi({ '/api/me': NOT_SIGNED_IN, '/api/setup': { needed: false } });
    renderApp('/admin');
    expect(await screen.findByLabelText('Username')).toBeInTheDocument();
    expect(screen.getByTestId('where')).toHaveTextContent('/login?next=%2Fadmin');
  });

  it('an admin sees only the admin area: no data tabs, no sync or watch', async () => {
    mockApi(adminApi());
    renderApp('/admin');
    expect(await screen.findByRole('heading', { level: 1, name: 'Admin' })).toBeInTheDocument();
    expect(screen.queryByRole('navigation', { name: 'Main' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Refresh' })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /Watch/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Activities' })).not.toBeInTheDocument();
  });

  it.each(['/', '/activities', '/sleep', '/devices', '/profile/phones', '/profile/garmin', '/does-not-exist'])('an admin opening %s is sent to /admin, without a data request', async (path) => {
    const { calls } = mockApi(adminApi());
    renderApp(path);
    expect(await screen.findByRole('heading', { level: 1, name: 'Admin' })).toBeInTheDocument();
    expect(screen.getByTestId('where')).toHaveTextContent(/^\/admin$/);
    expect(calls.filter((c) => /^\/api\/(today|activities|settings|tokens|sleep|garmin|devices)/.test(c.url))).toEqual([]);
  });

  it('a normal user opening /admin gets the not-found page, and loads nothing of the admin API', async () => {
    const { calls } = mockApi({ '/api/me': me() });
    renderApp('/admin');
    expect(await screen.findByRole('heading', { level: 1, name: 'Page not found' })).toBeInTheDocument();
    expect(calls.some((c) => c.url.startsWith('/api/admin'))).toBe(false);
  });

  it('a normal user sees the data pages with the tabs', async () => {
    mockApi({ '/api/me': me(), '/api/settings': { timezone: 'Europe/Copenhagen', step_goal: 8000, intensity_goal_min: 150, home_cards: { order: [], hidden: [] }, sample: false }, '/api/tokens': { tokens: [] }, '/api/garmin': { state: 'off', pending: 0, done: 0, failed: 0, last_error: null, next_attempt_at: null }, '/api/about': { version: '0.1.0' } });
    renderApp('/profile');
    expect(await screen.findByRole('heading', { level: 1, name: 'Profile' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Password' })).toHaveAttribute('href', '/profile/password');
    expect(screen.getByRole('link', { name: /^Timezone/ })).toHaveAttribute('href', '/profile/timezone');
  });

  it('/settings is an old address for /profile', async () => {
    mockApi({ '/api/me': me(), '/api/settings': { timezone: 'Europe/Copenhagen', step_goal: 8000, intensity_goal_min: 150, home_cards: { order: [], hidden: [] }, sample: false }, '/api/tokens': { tokens: [] }, '/api/garmin': { state: 'off', pending: 0, done: 0, failed: 0, last_error: null, next_attempt_at: null }, '/api/about': { version: '0.1.0' } });
    renderApp('/settings');
    expect(await screen.findByRole('heading', { level: 1, name: 'Profile' })).toBeInTheDocument();
    expect(screen.getByTestId('where')).toHaveTextContent(/^\/profile$/);
  });

  it('an admin opening /settings lands on the profile, which holds only Account, the admin area and Log out, and asks for no data', async () => {
    const { calls } = mockApi(adminApi({ '/api/about': { version: '0.1.0' } }));
    renderApp('/settings');
    expect(await screen.findByRole('heading', { level: 1, name: 'Profile' })).toBeInTheDocument();
    expect(screen.getByTestId('where')).toHaveTextContent(/^\/profile$/);
    expect(screen.getByRole('link', { name: 'Password' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /^Two-step login/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Admin area' })).toHaveAttribute('href', '/admin');
    expect(screen.getByRole('button', { name: 'Log out' })).toBeInTheDocument();
    for (const name of [/Timezone/, /Goals/, /Signed-in phones/, /Send activities/]) expect(screen.queryByRole('link', { name })).not.toBeInTheDocument();
    expect(screen.queryByRole('navigation', { name: 'Profile sections' })).not.toBeInTheDocument();
    expect(calls.filter((c) => /^\/api\/(today|activities|settings|tokens|sleep|garmin|devices)/.test(c.url))).toEqual([]);
  });
});

describe('profile link', () => {
  it('the avatar is a link called Profile, on the phone bar and the desktop header', async () => {
    mockApi({ '/api/me': me(), '/api/setup': { needed: false } });
    renderApp('/devices');
    const links = await screen.findAllByRole('link', { name: 'Profile' });
    expect(links.length).toBeGreaterThan(0);
    for (const link of links) expect(link).toHaveAttribute('href', '/profile');
    expect(screen.queryByRole('button', { name: 'Profile and account' })).not.toBeInTheDocument();
  });

  it('an admin has it too, and it leads to their profile', async () => {
    mockApi(adminApi({ '/api/about': { version: '0.1.0' } }));
    const u = userEvent.setup();
    renderApp('/admin');
    await u.click((await screen.findAllByRole('link', { name: 'Profile' }))[0]!);
    expect(await screen.findByRole('heading', { level: 1, name: 'Profile' })).toBeInTheDocument();
    expect(screen.getByText('boss')).toBeInTheDocument();
  });

  it('Log out on the profile asks first, then ends the session and goes to the login page', async () => {
    const { calls } = mockApi({ '/api/me': me(), 'POST /api/logout': () => new Response(null, { status: 204 }), '/api/setup': { needed: false }, '/api/settings': { timezone: 'Europe/Copenhagen', step_goal: 8000, intensity_goal_min: 150, home_cards: { order: [], hidden: [] }, sample: false }, '/api/tokens': { tokens: [] }, '/api/garmin': { state: 'off', pending: 0, done: 0, failed: 0, last_error: null, next_attempt_at: null }, '/api/about': { version: '0.1.0' } });
    const u = userEvent.setup();
    renderApp('/profile');
    await u.click(await screen.findByRole('button', { name: 'Log out' }));
    const dialog = await screen.findByRole('dialog', { name: 'Log out?' });
    expect(calls.some((c) => c.url === '/api/logout')).toBe(false);
    await u.click(within(dialog).getByRole('button', { name: 'Log out' }));
    await waitFor(() => expect(screen.getByTestId('where')).toHaveTextContent('/login'));
    expect(calls.some((c) => c.url === '/api/logout' && c.init?.method === 'POST')).toBe(true);
  });
});
