import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../../App';
import type { GarminStatus, SettingsResponse, TokenList } from '../../api/generated';
import { json, mockApi } from '../body/testUtils';
import { parseTokenFile } from './GarminPage';
import { formFrom, validateGoals } from './goals';
import { navKeyForProfile } from './ProfileLayout';
import { activePhones, phonesSummary } from './common';
import { phoneLine } from './PhonesPage';
import { sectionFromSearch } from './ProfilePage';

const settings: SettingsResponse = {
  timezone: 'Europe/Copenhagen',
  step_goal: 8000,
  intensity_goal_min: 150,
  home_cards: { shown: [], hidden: [] } as unknown as SettingsResponse['home_cards'],
  sample: false,
};

const me = { authenticated: true, username: 'demo', role: 'user', timezone: 'Europe/Copenhagen', sample: false };
const garmin = (over: Partial<GarminStatus> = {}): GarminStatus => ({ state: 'off', pending: 0, done: 0, failed: 0, last_error: null, next_attempt_at: null, ...over });

const phone = (over: Record<string, unknown> = {}) => ({
  id: 1,
  name: 'Pixel 9a',
  created_at: '2026-11-02T10:00:00+01:00',
  last_used_at: '2026-11-19T07:42:00+01:00',
  revoked_at: null,
  active: true,
  current: false,
  ...over,
});

const tokens = (): TokenList => ({
  tokens: [phone(), { ...phone({ id: 2, name: 'Old phone', created_at: '2026-10-01T10:00:00+01:00', last_used_at: null, revoked_at: '2026-10-20T09:00:00+01:00', active: false }) }] as TokenList['tokens'],
});

function Where() {
  const l = useLocation();
  return <div data-testid="where">{l.pathname + l.search}</div>;
}

function renderApp(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <App />
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function routes(over: Record<string, unknown> = {}) {
  return { '/api/me': me, '/api/settings': settings, '/api/tokens': tokens(), '/api/garmin': garmin(), '/api/about': { version: '0.1.0' }, ...over };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('Profile: the list', () => {
  it('shows who is signed in and the settings as rows with their current values', async () => {
    mockApi(routes({ '/api/tokens': { tokens: [phone({ current: true }), phone({ id: 3, name: 'Pixel 8a' })] } }));
    renderApp('/profile');
    expect(await screen.findByRole('heading', { level: 1, name: 'Profile' })).toBeInTheDocument();
    const head = screen.getByRole('region', { name: 'Your account' });
    expect(head).toHaveTextContent('demo');
    expect(head).toHaveTextContent('Two-step login on');
    // The server's host name, not an address with a scheme.
    expect(head).toHaveTextContent(window.location.host);
    expect(head).not.toHaveTextContent('http');

    const link = (name: string | RegExp, href: string) => expect(screen.getByRole('link', { name })).toHaveAttribute('href', href);
    link('Password', '/profile/password');
    link('Two-step login On', '/profile/two-step');
    expect(await screen.findByRole('link', { name: 'Signed-in phones Pixel 9a (this phone) and 1 more' })).toHaveAttribute('href', '/profile/phones');
    link('Timezone Europe/Copenhagen', '/profile/timezone');
    link('Goals 8,000 steps · 150 intensity min', '/profile/goals');
    link('Send activities to Garmin Off', '/profile/garmin');
    expect(screen.getByText('Server 0.1.0 · AGPL-3.0')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /^Source code/ })).toHaveAttribute('href', 'https://github.com/AngelFreak/Nimrag-Connect');
    expect(screen.getByRole('button', { name: 'Log out' })).toBeInTheDocument();
  });

  it('offers "Download my data" as a plain same-tab download link to /api/export, for a user and not for the admin', async () => {
    mockApi(routes());
    const { unmount } = renderApp('/profile');
    const link = await screen.findByRole('link', { name: /^Download my data/ });
    expect(link).toHaveAttribute('href', '/api/export');
    expect(link).toHaveAttribute('download');
    // Same tab: the app's web view hands the download to the system, a new tab would not be followed.
    expect(link).not.toHaveAttribute('target');
    unmount();

    mockApi(routes({ '/api/me': { ...me, username: 'boss', role: 'admin' } }));
    renderApp('/profile');
    await screen.findByRole('link', { name: 'Admin area' });
    expect(screen.queryByRole('link', { name: /Download my data/ })).not.toBeInTheDocument();
  });

  it('groups the rows Account, Preferences, Garmin Connect, About, Log out, with no form on the list', async () => {
    mockApi(routes());
    renderApp('/profile');
    await screen.findByRole('link', { name: /^Timezone/ });
    const titles = Array.from(document.querySelectorAll('.sgroup__title')).map((h) => h.textContent);
    expect(titles).toEqual(['Account', 'Preferences', 'Garmin Connect', 'About', 'Log out']);
    expect(screen.queryByLabelText('Current password')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Save/ })).not.toBeInTheDocument();
  });

  it('leaves the About version out when the server does not say', async () => {
    mockApi(routes({ '/api/about': () => json({ error: 'nope' }, { status: 500 }) }));
    renderApp('/profile');
    expect(await screen.findByText('AGPL-3.0')).toBeInTheDocument();
    expect(screen.queryByText(/Server \d/)).not.toBeInTheDocument();
  });

  it('has a section list for desktop with the five sections, the current one marked', async () => {
    mockApi(routes());
    renderApp('/profile?section=preferences');
    const nav = await screen.findByRole('navigation', { name: 'Profile sections' });
    expect(within(nav).getAllByRole('link').map((a) => a.textContent)).toEqual(['Account', 'Preferences', 'Phones', 'Garmin Connect', 'About']);
    expect(within(nav).getByRole('link', { name: 'Preferences' })).toHaveAttribute('aria-current', 'page');
    expect(within(nav).getByRole('link', { name: 'Phones' })).toHaveAttribute('href', '/profile/phones');
  });

  it('navKeyForProfile and sectionFromSearch map a page to its section', () => {
    expect(navKeyForProfile('/profile', '')).toBe('account');
    expect(navKeyForProfile('/profile', '?section=about')).toBe('about');
    expect(navKeyForProfile('/profile/password', '')).toBe('account');
    expect(navKeyForProfile('/profile/two-step', '')).toBe('account');
    expect(navKeyForProfile('/profile/timezone', '')).toBe('preferences');
    expect(navKeyForProfile('/profile/goals', '')).toBe('preferences');
    expect(navKeyForProfile('/profile/phones', '')).toBe('phones');
    expect(navKeyForProfile('/profile/garmin', '')).toBe('garmin');
    expect(sectionFromSearch('?section=nonsense')).toBe('account');
  });
});

describe('Profile: password', () => {
  const meOf = { step: 'done', ...me };

  it('is its own small page, sends the current and the new password, and goes back with a notice', async () => {
    const { calls } = mockApi(routes({ 'POST /api/me/password': () => json(meOf) }));
    const u = userEvent.setup();
    renderApp('/profile');
    await u.click(await screen.findByRole('link', { name: 'Password' }));
    expect(await screen.findByRole('heading', { level: 1, name: 'Password' })).toBeInTheDocument();
    const save = screen.getByRole('button', { name: 'Change password' });
    expect(save).toBeDisabled();
    await u.type(screen.getByLabelText('Current password'), 'old password!');
    await u.type(screen.getByLabelText('New password'), 'new password 123');
    await u.type(screen.getByLabelText('New password again'), 'new password 124');
    expect(await screen.findByText('The two passwords are not the same.')).toBeInTheDocument();
    expect(save).toBeDisabled();
    await u.clear(screen.getByLabelText('New password again'));
    await u.type(screen.getByLabelText('New password again'), 'new password 123');
    await u.click(save);

    expect(JSON.parse(String(calls.find((x) => x.url === '/api/me/password')?.init?.body))).toEqual({ current_password: 'old password!', new_password: 'new password 123' });
    expect(await screen.findByRole('heading', { level: 1, name: 'Profile' })).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Password changed. Your other devices were logged out.');
    expect(screen.getByTestId('where')).toHaveTextContent(/^\/profile$/);
  });

  it('a wrong current password (403) says so and stays on the page', async () => {
    mockApi(routes({ 'POST /api/me/password': () => json({ error: 'wrong password' }, { status: 403 }) }));
    const u = userEvent.setup();
    renderApp('/profile/password');
    await u.type(await screen.findByLabelText('Current password'), 'nope nope nope');
    await u.type(screen.getByLabelText('New password'), 'new password 123');
    await u.type(screen.getByLabelText('New password again'), 'new password 123');
    await u.click(screen.getByRole('button', { name: 'Change password' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Wrong password.');
    expect(screen.getByTestId('where')).toHaveTextContent('/profile/password');
  });
});

describe('Profile: two-step login', () => {
  const start = { pending_token: 'pending-reenrol', otpauth_uri: 'otpauth://totp/Nimrag%20Connect:demo?secret=KRSXG5CTMVRXEZLU&issuer=Nimrag%20Connect', secret: 'KRSXG5CTMVRXEZLU' };

  it('asks for the password, shows the QR code and secret, then confirms with a code', async () => {
    const { calls } = mockApi(routes({ 'POST /api/me/2fa/confirm': () => new Response(null, { status: 204 }), 'POST /api/me/2fa': start }));
    const u = userEvent.setup();
    renderApp('/profile/two-step');
    await u.click(await screen.findByRole('button', { name: 'Set up again' }));
    expect(screen.queryByRole('img', { name: /QR code/ })).not.toBeInTheDocument();
    await u.type(screen.getByLabelText('Your password'), 'my password 123');
    await u.click(screen.getByRole('button', { name: 'Continue' }));

    expect(await screen.findByRole('img', { name: /QR code/ })).toBeInTheDocument();
    expect(screen.getByLabelText('Secret key')).toHaveValue('KRSXG5CTMVRXEZLU');
    expect(JSON.parse(String(calls.find((x) => x.url === '/api/me/2fa')?.init?.body))).toEqual({ password: 'my password 123' });
    const confirm = screen.getByRole('button', { name: 'Confirm' });
    expect(confirm).toBeDisabled();
    await u.type(screen.getByLabelText('6-digit code'), '123456');
    await u.click(confirm);
    expect(await screen.findByRole('status')).toHaveTextContent('Two-step login is set up with the new authenticator');
    expect(JSON.parse(String(calls.find((x) => x.url === '/api/me/2fa/confirm')?.init?.body))).toEqual({ pending_token: 'pending-reenrol', code: '123456' });
    expect(screen.queryByRole('img', { name: /QR code/ })).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to profile' })).toHaveAttribute('href', '/profile');
  });

  it('a wrong password (403) stays on the password step', async () => {
    mockApi(routes({ 'POST /api/me/2fa': () => json({ error: 'wrong password' }, { status: 403 }) }));
    const u = userEvent.setup();
    renderApp('/profile/two-step');
    await u.click(await screen.findByRole('button', { name: 'Set up again' }));
    await u.type(screen.getByLabelText('Your password'), 'nope');
    await u.click(screen.getByRole('button', { name: 'Continue' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Wrong password.');
    expect(screen.getByLabelText('Your password')).toHaveValue('');
    expect(screen.getByTestId('where')).toHaveTextContent('/profile/two-step');
  });

  it('a wrong code (422) can be tried again; an expired step (401) starts over without leaving the page', async () => {
    let attempt = 0;
    mockApi(
      routes({
        'POST /api/me/2fa/confirm': () => {
          attempt += 1;
          return attempt === 1 ? json({ error: 'wrong code' }, { status: 422 }) : json({ error: 'this step has expired; log in again' }, { status: 401 });
        },
        'POST /api/me/2fa': start,
      }),
    );
    const u = userEvent.setup();
    renderApp('/profile/two-step');
    await u.click(await screen.findByRole('button', { name: 'Set up again' }));
    await u.type(screen.getByLabelText('Your password'), 'my password 123');
    await u.click(screen.getByRole('button', { name: 'Continue' }));
    await u.type(await screen.findByLabelText('6-digit code'), '111111');
    await u.click(screen.getByRole('button', { name: 'Confirm' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('That code is not right');
    expect(screen.getByRole('img', { name: /QR code/ })).toBeInTheDocument();

    await u.type(screen.getByLabelText('6-digit code'), '222222');
    await u.click(screen.getByRole('button', { name: 'Confirm' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('That step expired. Start again.');
    expect(screen.getByRole('button', { name: 'Set up again' })).toBeInTheDocument();
    expect(screen.getByTestId('where')).toHaveTextContent('/profile/two-step');
  });
});

describe('Profile: signed-in phones', () => {
  it('lists the active phones with signed-in date and last upload, never a token', async () => {
    mockApi(routes());
    renderApp('/profile/phones');
    expect(await screen.findByText('Pixel 9a')).toBeInTheDocument();
    expect(screen.getByText(/Signed in 2 Nov, 10:00 · last upload 19 Nov, 07:42/)).toBeInTheDocument();
    expect(screen.getAllByRole('listitem').filter((li) => li.textContent?.includes('Sign out'))).toHaveLength(1);
    // A revoked phone is signed out already: it is not listed.
    expect(screen.queryByText('Old phone')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Create token' })).not.toBeInTheDocument();
    expect(screen.queryByText(/device token/i)).not.toBeInTheDocument();
  });

  it('marks the phone this page is open on, and tells two phones with one name apart by when they signed in', async () => {
    mockApi(
      routes({
        '/api/tokens': {
          tokens: [phone({ id: 1, name: 'Pixel 8a', created_at: '2026-10-01T10:00:00+01:00', last_used_at: null }), phone({ id: 2, name: 'Pixel 8a', created_at: '2026-11-05T09:30:00+01:00', current: true })],
        },
      }),
    );
    renderApp('/profile/phones');
    expect(await screen.findByText('Pixel 8a (this phone)')).toBeInTheDocument();
    expect(screen.getByText(/Signed in 1 Oct, 11:00 · no upload yet/)).toBeInTheDocument();
    expect(screen.getByText(/Signed in 5 Nov, 09:30 · last upload/)).toBeInTheDocument();
    // Each button names its phone and its sign-in date.
    expect(screen.getByRole('button', { name: 'Sign out Pixel 8a, signed in 1 Oct, 11:00' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign out Pixel 8a, signed in 5 Nov, 09:30' })).toBeInTheDocument();
  });

  it('with no phone signed in, says how a phone signs in', async () => {
    mockApi(routes({ '/api/tokens': { tokens: [] } }));
    renderApp('/profile/phones');
    expect(await screen.findByText(/No phone is signed in/)).toBeInTheDocument();
  });

  it('signing a phone out states the consequence, then calls DELETE only after the confirm', async () => {
    const { calls } = mockApi(routes({ 'DELETE /api/tokens/1': () => new Response(null, { status: 204 }) }));
    const u = userEvent.setup();
    renderApp('/profile/phones');
    await u.click(await screen.findByRole('button', { name: /^Sign out Pixel 9a/ }));
    const dialog = screen.getByRole('dialog', { name: 'Sign out Pixel 9a?' });
    expect(dialog).toHaveTextContent('has to sign in again with your username, password and a code');
    expect(calls.some((c) => c.init?.method === 'DELETE')).toBe(false);
    await u.click(within(dialog).getByRole('button', { name: 'Sign out' }));
    await waitFor(() => expect(calls.some((c) => c.init?.method === 'DELETE' && c.url === '/api/tokens/1')).toBe(true));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  });

  it('cancelling the confirmation changes nothing', async () => {
    const { calls } = mockApi(routes());
    const u = userEvent.setup();
    renderApp('/profile/phones');
    await u.click(await screen.findByRole('button', { name: /^Sign out Pixel 9a/ }));
    await u.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(calls.some((c) => c.init?.method === 'DELETE')).toBe(false);
  });

  it('phonesSummary puts this phone first, counts the rest, and ignores signed-out phones', () => {
    const list = tokens().tokens;
    expect(phonesSummary(list)).toBe('Pixel 9a');
    expect(phonesSummary([...list, phone({ id: 3, name: 'Pixel 8a', last_used_at: null }) as TokenList['tokens'][number]])).toBe('Pixel 9a and 1 more');
    expect(phonesSummary([phone({ id: 3, name: 'Pixel 8a', current: true }), ...list] as TokenList['tokens'])).toBe('Pixel 8a (this phone) and 1 more');
    expect(phonesSummary([])).toBe('None signed in');
    expect(activePhones(undefined)).toEqual([]);
    expect(phoneLine(phone() as TokenList['tokens'][number], 'Europe/Copenhagen')).toBe('Signed in 2 Nov, 10:00 · last upload 19 Nov, 07:42');
  });
});

describe('Profile: timezone', () => {
  it('picks the zone from a list of IANA zones and saves at once, with no Save button', async () => {
    const { calls } = mockApi(routes({ 'PATCH /api/settings': { ...settings, timezone: 'Europe/Paris' } }));
    const u = userEvent.setup();
    renderApp('/profile/timezone');
    const field = await screen.findByLabelText('Timezone');
    expect(field.tagName).toBe('SELECT');
    expect(field).toHaveValue('Europe/Copenhagen');
    expect(within(field).getByRole('option', { name: 'UTC' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Save/ })).not.toBeInTheDocument();
    await u.selectOptions(field, 'Europe/Paris');
    expect(await screen.findByRole('status')).toHaveTextContent('Saved.');
    expect(JSON.parse(String(calls.find((c) => c.init?.method === 'PATCH')?.init?.body))).toEqual({ timezone: 'Europe/Paris' });
  });

  it('shows the server message when it refuses, and goes back to the saved zone', async () => {
    mockApi(routes({ 'PATCH /api/settings': () => json({ error: 'timezone must be an IANA name such as Europe/Copenhagen' }, { status: 400 }) }));
    const u = userEvent.setup();
    renderApp('/profile/timezone');
    const field = await screen.findByLabelText('Timezone');
    await u.selectOptions(field, 'Europe/Paris');
    expect(await screen.findByText(/must be an IANA name/)).toBeInTheDocument();
    await waitFor(() => expect(field).toHaveValue('Europe/Copenhagen'));
  });

  it('keeps a saved zone the browser does not list ("Etc/UTC") selectable', async () => {
    mockApi(routes({ '/api/settings': { ...settings, timezone: 'Etc/UTC' } }));
    renderApp('/profile/timezone');
    expect(await screen.findByLabelText('Timezone')).toHaveValue('Etc/UTC');
  });

  it('offers a one-tap "Use <browser zone>" when the saved zone differs, and saves it', async () => {
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({ timeZone: 'Europe/Copenhagen' } as Intl.ResolvedDateTimeFormatOptions);
    const { calls } = mockApi(routes({ '/api/settings': { ...settings, timezone: 'Etc/UTC' }, 'PATCH /api/settings': settings }));
    const u = userEvent.setup();
    renderApp('/profile/timezone');
    await u.click(await screen.findByRole('button', { name: 'Use Europe/Copenhagen' }));
    await waitFor(() => expect(calls.find((c) => c.init?.method === 'PATCH')).toBeDefined());
    expect(JSON.parse(String(calls.find((c) => c.init?.method === 'PATCH')?.init?.body))).toEqual({ timezone: 'Europe/Copenhagen' });
  });

  it('has no "Use" button when the saved zone is the browser zone', async () => {
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({ timeZone: 'Europe/Copenhagen' } as Intl.ResolvedDateTimeFormatOptions);
    mockApi(routes());
    renderApp('/profile/timezone');
    await screen.findByLabelText('Timezone');
    expect(screen.queryByRole('button', { name: /^Use / })).not.toBeInTheDocument();
  });
});

describe('Profile: goals', () => {
  it('saves a changed goal with PATCH /api/settings and says so', async () => {
    const saved = { ...settings, step_goal: 10000 };
    const { calls } = mockApi(routes({ 'PATCH /api/settings': saved }));
    const u = userEvent.setup();
    renderApp('/profile/goals');
    const field = await screen.findByLabelText('Daily step goal');
    expect(field).toHaveValue('8000');
    expect(screen.getByLabelText('Weekly intensity minutes goal')).toHaveValue('150');
    const save = screen.getByRole('button', { name: 'Save goals' });
    expect(save).toBeDisabled();
    await u.clear(field);
    await u.type(field, '10000');
    await u.click(save);
    expect(await screen.findByRole('status')).toHaveTextContent('Saved.');
    const patch = calls.find((c) => c.init?.method === 'PATCH');
    expect(patch?.url).toBe('/api/settings');
    expect(JSON.parse(String(patch?.init?.body))).toEqual({ step_goal: 10000 });
    expect((patch?.init?.headers as Record<string, string>)['X-Nimrag']).toBe('1');
  });

  it('shows the field error and sends nothing for a bad value', async () => {
    const { calls } = mockApi(routes());
    const u = userEvent.setup();
    renderApp('/profile/goals');
    const field = await screen.findByLabelText('Weekly intensity minutes goal');
    await u.clear(field);
    await u.type(field, 'many');
    await u.click(screen.getByRole('button', { name: 'Save goals' }));
    expect(field).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByText(/whole number of minutes/)).toBeInTheDocument();
    expect(calls.some((c) => c.init?.method === 'PATCH')).toBe(false);
  });

  it('a failed settings request offers a retry', async () => {
    mockApi(routes({ '/api/settings': () => json({ error: 'database busy' }, { status: 500 }) }));
    renderApp('/profile/goals');
    expect(await screen.findByText('Could not load the settings')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
  });
});

describe('goal validation', () => {
  it('sends only the fields that changed', () => {
    const form = { ...formFrom(settings), steps: '9000' };
    expect(validateGoals(form, settings)).toEqual({ errors: {}, patch: { step_goal: 9000 } });
  });

  it('refuses a bad timezone, non-numbers and out-of-range goals', () => {
    const r = validateGoals({ timezone: 'Mars/Olympus', steps: '0', intensity: '1.5' }, settings);
    expect(r.patch).toBeNull();
    expect(Object.keys(r.errors).sort()).toEqual(['intensity', 'steps', 'timezone']);
    expect(validateGoals({ ...formFrom(settings), steps: '200001' }, settings).errors.steps).toBeDefined();
    expect(validateGoals({ ...formFrom(settings), intensity: '10081' }, settings).errors.intensity).toBeDefined();
  });

  it('has nothing to send when nothing changed', () => {
    expect(validateGoals(formFrom(settings), settings)).toEqual({ errors: {}, patch: null });
  });
});

describe('Profile: send activities to Garmin', () => {
  it('off: two short lines and a switch; the warning and the login appear only after turning it on', async () => {
    mockApi(routes());
    const u = userEvent.setup();
    renderApp('/profile/garmin');
    const toggle = await screen.findByRole('switch', { name: /Send activities to Garmin/ });
    expect(toggle).not.toBeChecked();
    expect(screen.getByText(/Activities only/)).toBeInTheDocument();
    expect(screen.queryByLabelText('Garmin email')).not.toBeInTheDocument();
    expect(screen.queryByText(/unofficial/)).not.toBeInTheDocument();
    await u.click(toggle);
    expect(toggle).toBeChecked();
    expect(screen.getByText(/unofficial/)).toBeInTheDocument();
    expect(screen.getByText(/may stop working/)).toBeInTheDocument();
    expect(screen.getByLabelText('Garmin email')).toBeInTheDocument();
    expect(screen.getByLabelText('Garmin password')).toHaveAttribute('type', 'password');
    // Turning it off again hides the form without asking: nothing was connected.
    await u.click(toggle);
    expect(screen.queryByLabelText('Garmin email')).not.toBeInTheDocument();
  });

  it.each([
    ['needs_login', 'Garmin asks you to log in again.'],
    ['paused', 'Sending is paused: Garmin kept refusing the stored login.'],
  ] as const)('%s: one line and one fix; the form opens on that fix', async (state, line) => {
    mockApi(routes({ '/api/garmin': garmin({ state, last_error: 'Garmin refused the login' }) }));
    const u = userEvent.setup();
    renderApp('/profile/garmin');
    expect(await screen.findByText(line)).toBeInTheDocument();
    expect(screen.queryByLabelText('Garmin email')).not.toBeInTheDocument();
    expect(screen.getByText(/Last error: Garmin refused the login/)).toBeInTheDocument();
    await u.click(screen.getByRole('button', { name: 'Log in again' }));
    expect(screen.getByLabelText('Garmin email')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Log in to Garmin again' })).toBeInTheDocument();
  });

  it('logs in with email and password, never keeping the password in the field, then asks for the code', async () => {
    let status = garmin();
    const { calls } = mockApi(
      routes({
        '/api/garmin': () => json(status),
        'POST /api/garmin/login': () => {
          status = garmin({ state: 'needs_mfa' });
          return json({ mfa_required: true });
        },
        'POST /api/garmin/mfa': () => {
          status = garmin({ state: 'connected', done: 4 });
          return json(status);
        },
      }),
    );
    const u = userEvent.setup();
    renderApp('/profile/garmin');
    await u.click(await screen.findByRole('switch', { name: /Send activities to Garmin/ }));
    await u.type(screen.getByLabelText('Garmin email'), 'me@example.com');
    await u.type(screen.getByLabelText('Garmin password'), 's3cret!');
    await u.click(screen.getByRole('button', { name: 'Log in to Garmin' }));

    const login = calls.find((c) => c.url === '/api/garmin/login')!;
    expect(JSON.parse(String(login.init?.body))).toEqual({ email: 'me@example.com', password: 's3cret!' });

    const code = await screen.findByLabelText('Verification code');
    await u.type(code, '123456');
    await u.click(screen.getByRole('button', { name: 'Send code' }));
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/garmin/mfa')?.init?.body))).toEqual({ code: '123456' });

    expect(await screen.findByText('Waiting to send')).toBeInTheDocument();
    expect(screen.getByRole('switch', { name: /Send activities to Garmin/ })).toBeChecked();
    expect(screen.queryByLabelText('Garmin password')).not.toBeInTheDocument();
  });

  it('a refused login shows the server message and empties the password', async () => {
    mockApi(routes({ 'POST /api/garmin/login': () => json({ error: 'Garmin rejected the email or password' }, { status: 422 }) }));
    const u = userEvent.setup();
    renderApp('/profile/garmin');
    await u.click(await screen.findByRole('switch', { name: /Send activities to Garmin/ }));
    await u.type(screen.getByLabelText('Garmin email'), 'me@example.com');
    await u.type(screen.getByLabelText('Garmin password'), 'bad');
    await u.click(screen.getByRole('button', { name: 'Log in to Garmin' }));
    expect(await screen.findByText('Garmin rejected the email or password')).toBeInTheDocument();
    expect(screen.getByLabelText('Garmin password')).toHaveValue('');
  });

  it('keeps the token-file option: imports a file and sends only the token fields', async () => {
    const { calls } = mockApi(routes({ 'POST /api/garmin/import-tokens': garmin({ state: 'connected' }) }));
    const u = userEvent.setup();
    renderApp('/profile/garmin');
    await u.click(await screen.findByRole('switch', { name: /Send activities to Garmin/ }));
    const input = screen.getByLabelText('Garmin token file', { selector: 'input' });
    const file = new File([JSON.stringify({ di_token: 'aaa', di_refresh_token: 'bbb', di_client_id: 'cid', extra: 'ignored' })], 'garmin_tokens.json', {
      type: 'application/json',
    });
    await u.upload(input, file);
    await waitFor(() => expect(calls.some((c) => c.url === '/api/garmin/import-tokens')).toBe(true));
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/garmin/import-tokens')?.init?.body))).toEqual({
      di_token: 'aaa',
      di_refresh_token: 'bbb',
      di_client_id: 'cid',
    });
  });

  it('refuses a file that is not a token file without calling the server', async () => {
    const { calls } = mockApi(routes());
    const u = userEvent.setup();
    renderApp('/profile/garmin');
    await u.click(await screen.findByRole('switch', { name: /Send activities to Garmin/ }));
    await u.upload(screen.getByLabelText('Garmin token file', { selector: 'input' }), new File(['{"hello": 1}'], 'other.json', { type: 'application/json' }));
    expect(await screen.findByText(/not a python-garminconnect token file/)).toBeInTheDocument();
    expect(calls.some((c) => c.url.includes('import-tokens'))).toBe(false);
  });

  it('parseTokenFile accepts the python-garminconnect file and nothing else', () => {
    expect(parseTokenFile('{"di_token":"a","di_refresh_token":"b"}')).toEqual({ di_token: 'a', di_refresh_token: 'b', di_client_id: null });
    expect(parseTokenFile('{"di_token":"a"}')).toBeNull();
    expect(parseTokenFile('{"di_token":"","di_refresh_token":"b"}')).toBeNull();
    expect(parseTokenFile('not json')).toBeNull();
    expect(parseTokenFile('[]')).toBeNull();
  });

  it('connected: shows the queue counts, retries failures, backfills, and turning it off asks first', async () => {
    let status = garmin({ state: 'connected', pending: 2, done: 40, failed: 1, last_error: 'Garmin said 500', next_attempt_at: '2026-11-19T12:30:00+01:00' });
    const { calls } = mockApi(
      routes({
        '/api/garmin': () => json(status),
        'POST /api/garmin/retry': () => {
          status = { ...status, failed: 0, pending: 3 };
          return json(status);
        },
        'POST /api/garmin/backfill': { queued: 7 },
        'POST /api/garmin/logout': () => {
          status = garmin();
          return new Response(null, { status: 204 });
        },
      }),
    );
    const u = userEvent.setup();
    renderApp('/profile/garmin');
    const row = (label: string) => screen.getByText(label, { selector: 'dt' }).nextElementSibling as HTMLElement;
    await waitFor(() => expect(row('Waiting to send')).toHaveTextContent('2'));
    expect(screen.getByRole('switch', { name: /Send activities to Garmin/ })).toBeChecked();
    expect(row('Sent or already there')).toHaveTextContent('40');
    expect(row('Needs you')).toHaveTextContent('1');
    expect(row('Next try')).toHaveTextContent('19 Nov');
    expect(screen.getByText(/Last error: Garmin said 500/)).toBeInTheDocument();

    await u.click(screen.getByRole('button', { name: 'Try the failed ones again' }));
    await waitFor(() => expect(row('Needs you')).toHaveTextContent('0'));
    expect(screen.getByRole('button', { name: 'Try the failed ones again' })).toBeDisabled();

    const days = screen.getByLabelText('Also send activities from the last (days)');
    await u.clear(days);
    await u.type(days, '60');
    await u.click(screen.getByRole('button', { name: 'Send older activities' }));
    expect(await screen.findByText('Queued 7 files.')).toBeInTheDocument();
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/garmin/backfill')?.init?.body))).toEqual({ days: 60 });

    await u.click(screen.getByRole('switch', { name: /Send activities to Garmin/ }));
    const dialog = screen.getByRole('dialog', { name: 'Stop sending to Garmin?' });
    expect(dialog).toHaveTextContent('stored login tokens are deleted from your server');
    expect(calls.some((c) => c.url === '/api/garmin/logout')).toBe(false);
    await u.click(within(dialog).getByRole('button', { name: 'Turn off' }));
    await waitFor(() => expect(screen.getByRole('switch', { name: /Send activities to Garmin/ })).not.toBeChecked());
    expect(calls.some((c) => c.url === '/api/garmin/logout')).toBe(true);
  });

  it('cancelling the switch-off dialog keeps it on', async () => {
    mockApi(routes({ '/api/garmin': garmin({ state: 'connected' }) }));
    const u = userEvent.setup();
    renderApp('/profile/garmin');
    await u.click(await screen.findByRole('switch', { name: /Send activities to Garmin/ }));
    await u.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(screen.getByRole('switch', { name: /Send activities to Garmin/ })).toBeChecked();
  });

  it('refuses backfill days outside 1 to 3650', async () => {
    mockApi(routes({ '/api/garmin': garmin({ state: 'connected' }) }));
    const u = userEvent.setup();
    renderApp('/profile/garmin');
    const days = await screen.findByLabelText('Also send activities from the last (days)');
    await u.clear(days);
    await u.type(days, '4000');
    expect(screen.getByRole('button', { name: 'Send older activities' })).toBeDisabled();
    expect(days).toHaveAttribute('aria-invalid', 'true');
  });

  it('a failed status request offers a retry', async () => {
    mockApi(routes({ '/api/garmin': () => json({ error: 'database busy' }, { status: 500 }) }));
    renderApp('/profile/garmin');
    expect(await screen.findByText('Could not load the Garmin status')).toBeInTheDocument();
  });
});
