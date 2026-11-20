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

  it.each(['/', '/activities', '/settings', '/sleep', '/does-not-exist'])('an admin opening %s is sent to /admin, without a data request', async (path) => {
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
    mockApi({ '/api/me': me(), '/api/settings': { timezone: 'Europe/Copenhagen', step_goal: 8000, intensity_goal_min: 150, home_cards: { order: [], hidden: [] }, sample: false }, '/api/tokens': { tokens: [] }, '/api/garmin': { state: 'off', pending: 0, done: 0, failed: 0, last_error: null, next_attempt_at: null } });
    renderApp('/settings');
    expect(await screen.findByRole('heading', { level: 1, name: 'Settings' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Two-factor' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Change password' })).toBeInTheDocument();
  });
});

describe('profile menu', () => {
  it('shows the username and role, a link to Settings for a user, and logs out', async () => {
    const { calls } = mockApi({ '/api/me': me(), 'POST /api/logout': () => new Response(null, { status: 204 }), '/api/setup': { needed: false } });
    const u = userEvent.setup();
    renderApp('/devices');
    // The phone top bar and the desktop header each have one; the first is enough.
    await u.click((await screen.findAllByRole('button', { name: 'Profile and account' }))[0]!);
    const panel = await screen.findByRole('group', { name: 'Account' });
    expect(within(panel).getByText('demo')).toBeInTheDocument();
    expect(within(panel).getByText('User')).toBeInTheDocument();
    expect(within(panel).getByRole('link', { name: 'Settings' })).toHaveAttribute('href', '/settings');
    expect(within(panel).queryByRole('link', { name: 'Admin area' })).not.toBeInTheDocument();
    await u.click(within(panel).getByRole('button', { name: 'Log out' }));
    await waitFor(() => expect(screen.getByTestId('where')).toHaveTextContent('/login'));
    expect(calls.some((c) => c.url === '/api/logout' && c.init?.method === 'POST')).toBe(true);
  });

  it('shows an admin a link to /admin instead, and closes on Escape', async () => {
    mockApi(adminApi());
    const u = userEvent.setup();
    renderApp('/admin');
    const [button] = await screen.findAllByRole('button', { name: 'Profile and account' });
    await u.click(button!);
    const panel = await screen.findByRole('group', { name: 'Account' });
    expect(within(panel).getByText('boss')).toBeInTheDocument();
    expect(within(panel).getByText('Admin')).toBeInTheDocument();
    expect(within(panel).getByRole('link', { name: 'Admin area' })).toHaveAttribute('href', '/admin');
    expect(within(panel).queryByRole('link', { name: 'Settings' })).not.toBeInTheDocument();
    expect(button).toHaveAttribute('aria-expanded', 'true');
    await u.keyboard('{Escape}');
    expect(screen.queryByRole('group', { name: 'Account' })).not.toBeInTheDocument();
    expect(button).toHaveFocus();
  });
});
