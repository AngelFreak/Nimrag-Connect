import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DevicesResponse, GarminStatus, SettingsResponse, StorageResponse, TokenList } from '../../api/generated';
import { DevicesPage } from '../DevicesPage';
import { SettingsPage } from '../SettingsPage';
import { json, mockApi, renderAt } from '../body/testUtils';
import { parseTokenFile } from './GarminCard';
import { validateGoals, formFrom } from './GoalsCard';

const settings: SettingsResponse = {
  timezone: 'Europe/Copenhagen',
  step_goal: 8000,
  intensity_goal_min: 150,
  home_cards: { shown: [], hidden: [] } as unknown as SettingsResponse['home_cards'],
  sample: false,
};

const devices = (over: Partial<DevicesResponse> = {}): DevicesResponse => ({
  sample: false,
  watch: { model: null, firmware: null, battery_percent: 74, battery_at: '2026-11-19T07:42:00+01:00', last_data_at: '2026-11-19T07:40:00+01:00' },
  last_sync_at: '2026-11-19T07:42:00+01:00',
  new_files_24h: 5,
  upload_devices: [{ id: 1, name: 'Pixel 9a', created_at: '2026-11-02T10:00:00+01:00', last_used_at: '2026-11-19T07:42:00+01:00', active: true }],
  ...over,
});

const storage = (over: Partial<StorageResponse> = {}): StorageResponse => ({
  sample: false,
  files: 1284,
  bytes: 212 * 1024 * 1024,
  by_source: [{ source: 'watch', files: 1200, bytes: 200 * 1024 * 1024 }],
  parsed_ok: 1280,
  parsed_partial: 3,
  parsed_error: 1,
  database_bytes: 5 * 1024 * 1024,
  last_backup_at: null,
  ...over,
});

const garmin = (over: Partial<GarminStatus> = {}): GarminStatus => ({ state: 'off', pending: 0, done: 0, failed: 0, last_error: null, next_attempt_at: null, ...over });

const tokens = (): TokenList => ({
  tokens: [
    { id: 1, name: 'Pixel 9a', created_at: '2026-11-02T10:00:00+01:00', last_used_at: '2026-11-19T07:42:00+01:00', revoked_at: null, active: true },
    { id: 2, name: 'Old phone', created_at: '2026-10-01T10:00:00+01:00', last_used_at: null, revoked_at: '2026-10-20T09:00:00+01:00', active: false },
  ],
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  delete window.NimragApp;
});

describe('Devices page', () => {
  it('shows the watch, the server and the upload devices from the API', async () => {
    mockApi({ '/api/devices': devices(), '/api/storage': storage(), '/api/settings': settings });
    renderAt(<DevicesPage />, '/devices');
    expect(await screen.findByText('74%', { selector: '.stat__value' })).toBeInTheDocument();
    expect(screen.getByText('1,284 files, 212 MB')).toBeInTheDocument();
    expect(screen.getByText('5.0 MB')).toBeInTheDocument();
    expect(screen.getByText('Pixel 9a', { selector: 'td' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Manage signed-in phones' })).toHaveAttribute('href', '/settings');
    expect(screen.getByText('Read in part').nextElementSibling).toHaveTextContent('3');
  });

  it('shows "unavailable" for status it should know, and leaves out model, firmware, database and backup rows it does not', async () => {
    mockApi({
      '/api/devices': devices({ watch: { model: null, firmware: null, battery_percent: null, battery_at: null, last_data_at: null }, last_sync_at: null }),
      '/api/storage': storage({ database_bytes: null }),
      '/api/settings': settings,
    });
    renderAt(<DevicesPage />, '/devices');
    await screen.findByText('Latest watch data');
    const row = (label: string) => screen.getByText(label, { selector: 'dt' }).nextElementSibling as HTMLElement;
    for (const label of ['Model', 'Firmware', 'Database', 'Last backup']) expect(screen.queryByText(label, { selector: 'dt' })).not.toBeInTheDocument();
    expect(row('Battery')).toHaveTextContent('unavailable');
    expect(row('Last sync')).toHaveTextContent('unavailable');
    expect(screen.getByLabelText('Battery unavailable')).toBeInTheDocument();
  });

  it('says so when a request fails and offers a retry', async () => {
    let fail = true;
    mockApi({
      '/api/devices': () => (fail ? json({ error: 'database busy' }, { status: 500 }) : json(devices())),
      '/api/storage': storage(),
      '/api/settings': settings,
    });
    renderAt(<DevicesPage />, '/devices');
    expect(await screen.findAllByRole('alert')).not.toHaveLength(0);
    expect(screen.getAllByText('database busy').length).toBeGreaterThan(0);
    fail = false;
    await userEvent.click(screen.getAllByRole('button', { name: 'Try again' })[0]!);
    expect(await screen.findByText('74%', { selector: '.stat__value' })).toBeInTheDocument();
  });

  it('in the browser the sync button refreshes from the server; in the app it asks the watch', async () => {
    mockApi({ '/api/devices': devices(), '/api/storage': storage(), '/api/settings': settings });
    const { unmount } = renderAt(<DevicesPage />, '/devices');
    expect(await screen.findByRole('button', { name: 'Refresh from the server' })).toBeInTheDocument();
    unmount();
    const syncNow = vi.fn();
    window.NimragApp = { native: true, syncNow };
    renderAt(<DevicesPage />, '/devices');
    await userEvent.click(await screen.findByRole('button', { name: 'Sync now' }));
    expect(syncNow).toHaveBeenCalledTimes(1);
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

function settingsRoutes(over: Record<string, unknown> = {}) {
  return { '/api/settings': settings, '/api/tokens': tokens(), '/api/garmin': garmin(), ...over };
}

describe('Settings: timezone and goals', () => {
  it('saves a changed goal with PATCH /api/settings and says so', async () => {
    const saved = { ...settings, step_goal: 10000 };
    const { calls } = mockApi({ ...settingsRoutes(), 'PATCH /api/settings': saved });
    renderAt(<SettingsPage />, '/settings');
    const field = await screen.findByLabelText('Daily step goal');
    expect(field).toHaveValue('8000');
    const save = screen.getByRole('button', { name: 'Save settings' });
    expect(save).toBeDisabled();
    await userEvent.clear(field);
    await userEvent.type(field, '10000');
    await userEvent.click(save);
    expect(await screen.findByRole('status', {})).toHaveTextContent('Saved.');
    const patch = calls.find((c) => c.init?.method === 'PATCH');
    expect(patch?.url).toBe('/api/settings');
    expect(JSON.parse(String(patch?.init?.body))).toEqual({ step_goal: 10000 });
    expect((patch?.init?.headers as Record<string, string>)['X-Nimrag']).toBe('1');
  });

  it('shows the field error and sends nothing for a bad value', async () => {
    const { calls } = mockApi(settingsRoutes());
    renderAt(<SettingsPage />, '/settings');
    const field = await screen.findByLabelText('Weekly intensity minutes goal');
    await userEvent.clear(field);
    await userEvent.type(field, 'many');
    await userEvent.click(screen.getByRole('button', { name: 'Save settings' }));
    expect(field).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByText(/whole number of minutes/)).toBeInTheDocument();
    expect(calls.some((c) => c.init?.method === 'PATCH')).toBe(false);
  });

  it('shows the server message when it refuses', async () => {
    mockApi({ ...settingsRoutes(), 'PATCH /api/settings': () => json({ error: 'timezone must be an IANA name such as Europe/Copenhagen' }, { status: 400 }) });
    renderAt(<SettingsPage />, '/settings');
    const field = await screen.findByLabelText('Timezone');
    await userEvent.selectOptions(field, 'Europe/Paris');
    await userEvent.click(screen.getByRole('button', { name: 'Save settings' }));
    expect(await screen.findByText(/must be an IANA name/)).toBeInTheDocument();
  });

  it('picks the timezone from a list of IANA zones, not free text', async () => {
    mockApi(settingsRoutes());
    renderAt(<SettingsPage />, '/settings');
    const field = await screen.findByLabelText('Timezone');
    expect(field.tagName).toBe('SELECT');
    expect(field).toHaveValue('Europe/Copenhagen');
    expect(within(field).getByRole('option', { name: 'Europe/Paris' })).toBeInTheDocument();
    expect(within(field).getByRole('option', { name: 'UTC' })).toBeInTheDocument();
  });

  it('keeps a saved zone the browser does not list ("Etc/UTC") selectable', async () => {
    mockApi(settingsRoutes({ '/api/settings': { ...settings, timezone: 'Etc/UTC' } }));
    renderAt(<SettingsPage />, '/settings');
    expect(await screen.findByLabelText('Timezone')).toHaveValue('Etc/UTC');
  });

  it('offers a one-tap "Use <browser zone>" when the saved zone differs, and saves it', async () => {
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({ timeZone: 'Europe/Copenhagen' } as Intl.ResolvedDateTimeFormatOptions);
    const saved = { ...settings, timezone: 'Europe/Copenhagen' };
    const { calls } = mockApi({ ...settingsRoutes({ '/api/settings': { ...settings, timezone: 'Etc/UTC' } }), 'PATCH /api/settings': saved });
    renderAt(<SettingsPage />, '/settings');
    await userEvent.click(await screen.findByRole('button', { name: 'Use Europe/Copenhagen' }));
    await waitFor(() => expect(calls.find((c) => c.init?.method === 'PATCH')).toBeDefined());
    expect(JSON.parse(String(calls.find((c) => c.init?.method === 'PATCH')?.init?.body))).toEqual({ timezone: 'Europe/Copenhagen' });
  });

  it('has no "Use" button when the saved zone is the browser zone', async () => {
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({ timeZone: 'Europe/Copenhagen' } as Intl.ResolvedDateTimeFormatOptions);
    mockApi(settingsRoutes());
    renderAt(<SettingsPage />, '/settings');
    await screen.findByLabelText('Timezone');
    expect(screen.queryByRole('button', { name: /^Use / })).not.toBeInTheDocument();
  });
});

describe('Settings: signed-in phones', () => {
  it('lists the active phones with when they signed in and were last used, never a token', async () => {
    mockApi(settingsRoutes());
    renderAt(<SettingsPage />, '/settings');
    const list = await screen.findByRole('list', { name: 'Signed-in phones' });
    expect(within(list).getByText('Pixel 9a')).toBeInTheDocument();
    expect(within(list).getAllByRole('listitem')).toHaveLength(1);
    expect(within(list).getByText('Signed in')).toBeInTheDocument();
    expect(within(list).getByText('Last used')).toBeInTheDocument();
    expect(within(list).getByRole('button', { name: 'Sign out this phone, Pixel 9a' })).toHaveTextContent('Sign out this phone');
    // A revoked phone is signed out already: it is not listed.
    expect(within(list).queryByText('Old phone')).not.toBeInTheDocument();
    // Users do not create or copy tokens.
    expect(screen.queryByRole('button', { name: 'Create token' })).not.toBeInTheDocument();
    expect(screen.queryByText(/device token/i)).not.toBeInTheDocument();
  });

  it('with no phone signed in, says how a phone signs in', async () => {
    mockApi({ ...settingsRoutes(), '/api/tokens': { tokens: [] } });
    renderAt(<SettingsPage />, '/settings');
    expect(await screen.findByText(/No phone is signed in/)).toBeInTheDocument();
    expect(screen.queryByRole('list', { name: 'Signed-in phones' })).not.toBeInTheDocument();
  });

  it('signing a phone out asks once more, then calls DELETE', async () => {
    const { calls } = mockApi({ ...settingsRoutes(), 'DELETE /api/tokens/1': () => new Response(null, { status: 204 }) });
    renderAt(<SettingsPage />, '/settings');
    await userEvent.click(await screen.findByRole('button', { name: 'Sign out this phone, Pixel 9a' }));
    expect(calls.some((c) => c.init?.method === 'DELETE')).toBe(false);
    await userEvent.click(screen.getByRole('button', { name: 'Yes, sign it out, Pixel 9a' }));
    await waitFor(() => expect(calls.some((c) => c.init?.method === 'DELETE' && c.url === '/api/tokens/1')).toBe(true));
  });

  it('cancelling the confirmation changes nothing', async () => {
    const { calls } = mockApi(settingsRoutes());
    renderAt(<SettingsPage />, '/settings');
    await userEvent.click(await screen.findByRole('button', { name: 'Sign out this phone, Pixel 9a' }));
    await userEvent.click(screen.getByRole('button', { name: 'Cancel, Pixel 9a' }));
    expect(screen.getByRole('button', { name: 'Sign out this phone, Pixel 9a' })).toBeInTheDocument();
    expect(calls.some((c) => c.init?.method === 'DELETE')).toBe(false);
  });
});

describe('Settings: Garmin Connect (Forward)', () => {
  it('off: says activities only and unofficial, offers the login form', async () => {
    mockApi(settingsRoutes());
    renderAt(<SettingsPage />, '/settings');
    const card = (await screen.findByRole('heading', { name: 'Garmin Connect (Forward)' })).closest('section') as HTMLElement;
    expect(await within(card).findByText('Off')).toBeInTheDocument();
    expect(card).toHaveTextContent(/Activities only/);
    expect(card).toHaveTextContent(/unofficial/);
    expect(card).toHaveTextContent(/may stop working/);
    expect(within(card).getByLabelText('Garmin email')).toBeInTheDocument();
    expect(within(card).getByLabelText('Garmin password')).toHaveAttribute('type', 'password');
  });

  it.each(['needs_login', 'paused'] as const)('%s reads "Log in to Garmin again"', async (state) => {
    mockApi(settingsRoutes({ '/api/garmin': garmin({ state, last_error: 'Garmin refused the login' }) }));
    renderAt(<SettingsPage />, '/settings');
    const card = (await screen.findByRole('heading', { name: 'Garmin Connect (Forward)' })).closest('section') as HTMLElement;
    await waitFor(() => expect(within(card).getAllByText('Log in to Garmin again').length).toBeGreaterThanOrEqual(2));
    expect(within(card).getByRole('button', { name: 'Log in to Garmin again' })).toBeInTheDocument();
    expect(within(card).getByText(/Last error: Garmin refused the login/)).toBeInTheDocument();
  });

  it('logs in with email and password, never keeping the password in the field, then asks for the code', async () => {
    let status = garmin();
    const { calls } = mockApi({
      ...settingsRoutes(),
      '/api/garmin': () => json(status),
      'POST /api/garmin/login': () => {
        status = garmin({ state: 'needs_mfa' });
        return json({ mfa_required: true });
      },
      'POST /api/garmin/mfa': () => {
        status = garmin({ state: 'connected', done: 4 });
        return json(status);
      },
    });
    renderAt(<SettingsPage />, '/settings');
    await userEvent.type(await screen.findByLabelText('Garmin email'), 'me@example.com');
    await userEvent.type(screen.getByLabelText('Garmin password'), 's3cret!');
    await userEvent.click(screen.getByRole('button', { name: 'Log in to Garmin' }));

    const login = calls.find((c) => c.url === '/api/garmin/login')!;
    expect(JSON.parse(String(login.init?.body))).toEqual({ email: 'me@example.com', password: 's3cret!' });

    const code = await screen.findByLabelText('Verification code');
    await userEvent.type(code, '123456');
    await userEvent.click(screen.getByRole('button', { name: 'Send code' }));
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/garmin/mfa')?.init?.body))).toEqual({ code: '123456' });

    expect(await screen.findByText('Connected')).toBeInTheDocument();
    expect(screen.queryByLabelText('Garmin password')).not.toBeInTheDocument();
  });

  it('a refused login shows the server message', async () => {
    mockApi({ ...settingsRoutes(), 'POST /api/garmin/login': () => json({ error: 'Garmin rejected the email or password' }, { status: 422 }) });
    renderAt(<SettingsPage />, '/settings');
    await userEvent.type(await screen.findByLabelText('Garmin email'), 'me@example.com');
    await userEvent.type(screen.getByLabelText('Garmin password'), 'bad');
    await userEvent.click(screen.getByRole('button', { name: 'Log in to Garmin' }));
    expect(await screen.findByText('Garmin rejected the email or password')).toBeInTheDocument();
    expect(screen.getByLabelText('Garmin password')).toHaveValue('');
  });

  it('imports a token file and sends only the token fields', async () => {
    const { calls } = mockApi({
      ...settingsRoutes(),
      'POST /api/garmin/import-tokens': garmin({ state: 'connected' }),
    });
    renderAt(<SettingsPage />, '/settings');
    const input = await screen.findByLabelText('Garmin token file', { selector: 'input' });
    const file = new File([JSON.stringify({ di_token: 'aaa', di_refresh_token: 'bbb', di_client_id: 'cid', extra: 'ignored' })], 'garmin_tokens.json', {
      type: 'application/json',
    });
    await userEvent.upload(input, file);
    await waitFor(() => expect(calls.some((c) => c.url === '/api/garmin/import-tokens')).toBe(true));
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/garmin/import-tokens')?.init?.body))).toEqual({
      di_token: 'aaa',
      di_refresh_token: 'bbb',
      di_client_id: 'cid',
    });
  });

  it('refuses a file that is not a token file without calling the server', async () => {
    const { calls } = mockApi(settingsRoutes());
    renderAt(<SettingsPage />, '/settings');
    const input = await screen.findByLabelText('Garmin token file', { selector: 'input' });
    await userEvent.upload(input, new File(['{"hello": 1}'], 'other.json', { type: 'application/json' }));
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

  it('connected: shows the queue counts and next try, retries failures, backfills, logs out after a confirm', async () => {
    let status = garmin({ state: 'connected', pending: 2, done: 40, failed: 1, last_error: 'Garmin said 500', next_attempt_at: '2026-11-19T12:30:00+01:00' });
    const { calls } = mockApi({
      ...settingsRoutes(),
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
    });
    renderAt(<SettingsPage />, '/settings');
    const card = (await screen.findByRole('heading', { name: 'Garmin Connect (Forward)' })).closest('section') as HTMLElement;
    const row = (label: string) => within(card).getByText(label, { selector: 'dt' }).nextElementSibling as HTMLElement;
    await waitFor(() => expect(row('Waiting to send')).toHaveTextContent('2'));
    expect(row('Sent or already there')).toHaveTextContent('40');
    expect(row('Needs you')).toHaveTextContent('1');
    expect(row('Next try')).toHaveTextContent('19 Nov');
    expect(within(card).getByText(/Last error: Garmin said 500/)).toBeInTheDocument();

    await userEvent.click(within(card).getByRole('button', { name: 'Try the failed ones again' }));
    await waitFor(() => expect(row('Needs you')).toHaveTextContent('0'));
    expect(within(card).getByRole('button', { name: 'Try the failed ones again' })).toBeDisabled();

    const days = within(card).getByLabelText('Also send activities from the last (days)');
    await userEvent.clear(days);
    await userEvent.type(days, '60');
    await userEvent.click(within(card).getByRole('button', { name: 'Send older activities' }));
    expect(await within(card).findByText('Queued 7 files.')).toBeInTheDocument();
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/garmin/backfill')?.init?.body))).toEqual({ days: 60 });

    await userEvent.click(within(card).getByRole('button', { name: 'Log out of Garmin' }));
    expect(calls.some((c) => c.url === '/api/garmin/logout')).toBe(false);
    await userEvent.click(within(card).getByRole('button', { name: 'Yes, log out of Garmin' }));
    expect(await within(card).findByText('Off')).toBeInTheDocument();
  });

  it('refuses backfill days outside 1 to 3650', async () => {
    mockApi(settingsRoutes({ '/api/garmin': garmin({ state: 'connected' }) }));
    renderAt(<SettingsPage />, '/settings');
    const days = await screen.findByLabelText('Also send activities from the last (days)');
    await userEvent.clear(days);
    await userEvent.type(days, '4000');
    expect(screen.getByRole('button', { name: 'Send older activities' })).toBeDisabled();
    expect(days).toHaveAttribute('aria-invalid', 'true');
  });

  it('a failed status request offers a retry', async () => {
    mockApi(settingsRoutes({ '/api/garmin': () => json({ error: 'database busy' }, { status: 500 }) }));
    renderAt(<SettingsPage />, '/settings');
    expect(await screen.findByText('Could not load the Garmin status')).toBeInTheDocument();
  });
});

describe('Settings: session', () => {
  it('Log out posts to /api/logout and goes to the login page', async () => {
    const { calls } = mockApi({ ...settingsRoutes(), 'POST /api/logout': () => new Response(null, { status: 204 }) });
    renderAt(<SettingsPage />, '/settings');
    await userEvent.click(await screen.findByRole('button', { name: 'Log out' }));
    await waitFor(() => expect(screen.getByTestId('where')).toHaveTextContent('/login'));
    expect(calls.some((c) => c.url === '/api/logout')).toBe(true);
  });
});

describe('Settings: change password', () => {
  const meOf = { step: 'done', authenticated: true, username: 'demo', role: 'user', timezone: 'Europe/Copenhagen', sample: false };

  async function card() {
    return (await screen.findByRole('heading', { name: 'Change password' })).closest('section') as HTMLElement;
  }

  it('sends the current and the new password, then says the other devices were logged out', async () => {
    const { calls } = mockApi({ ...settingsRoutes(), 'POST /api/me/password': () => json(meOf) });
    renderAt(<SettingsPage />, '/settings');
    const c = within(await card());
    const save = c.getByRole('button', { name: 'Change password' });
    expect(save).toBeDisabled();
    await userEvent.type(c.getByLabelText('Current password'), 'old password!');
    await userEvent.type(c.getByLabelText('New password'), 'new password 123');
    await userEvent.type(c.getByLabelText('New password again'), 'new password 124');
    expect(await c.findByText('The two passwords are not the same.')).toBeInTheDocument();
    expect(save).toBeDisabled();
    await userEvent.clear(c.getByLabelText('New password again'));
    await userEvent.type(c.getByLabelText('New password again'), 'new password 123');
    await userEvent.click(save);
    expect(await c.findByRole('status')).toHaveTextContent('every other device was logged out');
    expect(JSON.parse(String(calls.find((x) => x.url === '/api/me/password')?.init?.body))).toEqual({ current_password: 'old password!', new_password: 'new password 123' });
    // The fields are emptied: no password stays in the page.
    expect(c.getByLabelText('Current password')).toHaveValue('');
    expect(c.getByLabelText('New password')).toHaveValue('');
  });

  it('a wrong current password (403) says so', async () => {
    mockApi({ ...settingsRoutes(), 'POST /api/me/password': () => json({ error: 'wrong password' }, { status: 403 }) });
    renderAt(<SettingsPage />, '/settings');
    const c = within(await card());
    await userEvent.type(c.getByLabelText('Current password'), 'nope nope nope');
    await userEvent.type(c.getByLabelText('New password'), 'new password 123');
    await userEvent.type(c.getByLabelText('New password again'), 'new password 123');
    await userEvent.click(c.getByRole('button', { name: 'Change password' }));
    expect(await c.findByRole('alert')).toHaveTextContent('Wrong password.');
    // Still on the settings page: 403 is not a lost session.
    expect(screen.getByTestId('where')).toHaveTextContent('/settings');
  });
});

describe('Settings: two-factor', () => {
  const start = { pending_token: 'pending-reenrol', otpauth_uri: 'otpauth://totp/Nimrag%20Connect:demo?secret=KRSXG5CTMVRXEZLU&issuer=Nimrag%20Connect', secret: 'KRSXG5CTMVRXEZLU' };

  async function card() {
    return (await screen.findByRole('heading', { name: 'Two-factor' })).closest('section') as HTMLElement;
  }

  it('asks for the password, shows the QR code and secret, then confirms with a code', async () => {
    const { calls } = mockApi({ ...settingsRoutes(), 'POST /api/me/2fa/confirm': () => new Response(null, { status: 204 }), 'POST /api/me/2fa': start });
    renderAt(<SettingsPage />, '/settings');
    const c = within(await card());
    await userEvent.click(c.getByRole('button', { name: 'Set up two-factor again' }));
    expect(c.queryByRole('img', { name: /QR code/ })).not.toBeInTheDocument();
    await userEvent.type(c.getByLabelText('Your password'), 'my password 123');
    await userEvent.click(c.getByRole('button', { name: 'Continue' }));

    expect(await c.findByRole('img', { name: /QR code/ })).toBeInTheDocument();
    expect(c.getByLabelText('Secret key')).toHaveValue('KRSXG5CTMVRXEZLU');
    expect(JSON.parse(String(calls.find((x) => x.url === '/api/me/2fa')?.init?.body))).toEqual({ password: 'my password 123' });
    const confirm = c.getByRole('button', { name: 'Confirm' });
    expect(confirm).toBeDisabled();
    await userEvent.type(c.getByLabelText('6-digit code'), '123456');
    await userEvent.click(confirm);
    expect(await c.findByRole('status')).toHaveTextContent('Two-factor is set up with the new authenticator');
    expect(JSON.parse(String(calls.find((x) => x.url === '/api/me/2fa/confirm')?.init?.body))).toEqual({ pending_token: 'pending-reenrol', code: '123456' });
    expect(c.queryByRole('img', { name: /QR code/ })).not.toBeInTheDocument();
  });

  it('a wrong password (403) stays on the password step', async () => {
    mockApi({ ...settingsRoutes(), 'POST /api/me/2fa': () => json({ error: 'wrong password' }, { status: 403 }) });
    renderAt(<SettingsPage />, '/settings');
    const c = within(await card());
    await userEvent.click(c.getByRole('button', { name: 'Set up two-factor again' }));
    await userEvent.type(c.getByLabelText('Your password'), 'nope');
    await userEvent.click(c.getByRole('button', { name: 'Continue' }));
    expect(await c.findByRole('alert')).toHaveTextContent('Wrong password.');
    expect(c.getByLabelText('Your password')).toHaveValue('');
    expect(screen.getByTestId('where')).toHaveTextContent('/settings');
  });

  it('a wrong code (422) can be tried again; an expired step (401) starts over without leaving the page', async () => {
    let attempt = 0;
    mockApi({
      ...settingsRoutes(),
      'POST /api/me/2fa/confirm': () => {
        attempt += 1;
        return attempt === 1 ? json({ error: 'wrong code' }, { status: 422 }) : json({ error: 'this step has expired; log in again' }, { status: 401 });
      },
      'POST /api/me/2fa': start,
    });
    renderAt(<SettingsPage />, '/settings');
    const c = within(await card());
    await userEvent.click(c.getByRole('button', { name: 'Set up two-factor again' }));
    await userEvent.type(c.getByLabelText('Your password'), 'my password 123');
    await userEvent.click(c.getByRole('button', { name: 'Continue' }));
    await userEvent.type(await c.findByLabelText('6-digit code'), '111111');
    await userEvent.click(c.getByRole('button', { name: 'Confirm' }));
    expect(await c.findByRole('alert')).toHaveTextContent('That code is not right');
    expect(c.getByRole('img', { name: /QR code/ })).toBeInTheDocument();

    await userEvent.type(c.getByLabelText('6-digit code'), '222222');
    await userEvent.click(c.getByRole('button', { name: 'Confirm' }));
    expect(await c.findByRole('alert')).toHaveTextContent('That step expired. Start again.');
    expect(c.getByRole('button', { name: 'Set up two-factor again' })).toBeInTheDocument();
    expect(screen.getByTestId('where')).toHaveTextContent('/settings');
  });
});

describe('Settings: sections', () => {
  it('groups the cards as Account, Timezone and goals, Phones, Garmin, in that order', async () => {
    mockApi(settingsRoutes());
    renderAt(<SettingsPage />, '/settings');
    await screen.findByRole('heading', { name: 'Garmin Connect (Forward)' });
    const titles = Array.from(document.querySelectorAll('.section__title')).map((h) => h.textContent);
    expect(titles).toEqual(['Account', 'Timezone and goals', 'Phones', 'Garmin']);
    // Cards sit under their section's title as h3, with the account cards first.
    const account = screen.getByRole('region', { name: 'Account' });
    expect(within(account).getAllByRole('heading', { level: 3 }).map((h) => h.textContent)).toEqual(['Signed in', 'Change password', 'Two-factor']);
  });

  it('the Garmin card still says the login is unofficial and the password is not stored', async () => {
    mockApi(settingsRoutes());
    renderAt(<SettingsPage />, '/settings');
    const card = (await screen.findByRole('heading', { name: 'Garmin Connect (Forward)' })).closest('section') as HTMLElement;
    expect(card).toHaveTextContent(/unofficial/);
    expect(card).toHaveTextContent(/never stored/);
  });
});
