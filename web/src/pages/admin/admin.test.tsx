import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AssignResponse, NewUserResponse, UnassignedResponse, UserInfo, UserList } from '../../api/generated';
import { AdminPage } from '../AdminPage';
import { json, mockApi, renderAt } from '../body/testUtils';
import { assignSummary, hasLegacy } from './UnassignedCard';
import { userFacts } from './UsersCard';

const user = (over: Partial<UserInfo> = {}): UserInfo => ({
  id: 2,
  username: 'demo',
  role: 'user',
  totp_enabled: true,
  must_change_password: false,
  disabled: false,
  created_at: '2026-11-02T10:00:00+01:00',
  active_tokens: 1,
  files: 1284,
  has_data: true,
  ...over,
});

const admin = user({ id: 1, username: 'admin', role: 'admin', active_tokens: 0, files: 0, has_data: false });

const users = (...list: UserInfo[]): UserList => ({ users: list });

const unassigned = (over: Partial<UnassignedResponse> = {}): UnassignedResponse => ({
  tokens: [{ id: 7, name: 'Pixel 9a', created_at: '2026-11-02T10:00:00+01:00', last_used_at: '2026-11-19T07:42:00+01:00', files: 312 }],
  unattributed_files: 4,
  legacy: { settings: true, body_entries: 3, strength_edits: 2, garmin_account: false },
  ...over,
});

const NOTHING: UnassignedResponse = { tokens: [], unattributed_files: 0, legacy: { settings: false, body_entries: 0, strength_edits: 0, garmin_account: false } };
const me = { authenticated: true, username: 'admin', role: 'admin', timezone: 'Europe/Copenhagen', sample: false };

function open(extra: Record<string, unknown> = {}, list: UserList = users(admin, user()), un: UnassignedResponse = NOTHING) {
  const api = mockApi({ ...extra, '/api/me': me, '/api/admin/users': list, '/api/admin/unassigned': un });
  renderAt(<AdminPage />, '/admin');
  return api;
}

/** The card, once its form (not the loading skeleton) is there. */
async function unassignedCard(): Promise<HTMLElement> {
  const form = await screen.findByRole('form', { name: 'Assign unassigned data' });
  return form.closest('section')!;
}

afterEach(() => vi.unstubAllGlobals());

describe('Admin: users', () => {
  it('lists every account with role, two-factor state, tokens, files and a disabled badge', async () => {
    open({}, users(admin, user(), user({ id: 3, username: 'bob', totp_enabled: false, must_change_password: true, disabled: true, files: 0, active_tokens: 0, has_data: false })));
    const list = await screen.findByRole('list', { name: 'Users' });
    const rows = within(list).getAllByRole('listitem');
    expect(rows).toHaveLength(3);
    expect(within(rows[1]!).getByText('demo')).toBeInTheDocument();
    expect(within(rows[1]!).getByText('Two-factor on · 1 active token · 1,284 files')).toBeInTheDocument();
    expect(within(rows[2]!).getByText('Disabled')).toBeInTheDocument();
    expect(within(rows[2]!).getByText('Two-factor not set up · temporary password · 0 active tokens · 0 files')).toBeInTheDocument();
    expect(userFacts(user({ active_tokens: 2, files: 1 }))).toBe('Two-factor on · 2 active tokens · 1 file');
  });

  it('hides the password and two-factor resets (and disable, delete) on the admin\'s own row', async () => {
    open();
    const list = await screen.findByRole('list', { name: 'Users' });
    const own = within(list).getAllByRole('listitem')[0]!;
    expect(within(own).getByText('You')).toBeInTheDocument();
    expect(within(own).queryByRole('button')).not.toBeInTheDocument();
    const other = within(list).getAllByRole('listitem')[1]!;
    expect(within(other).getAllByRole('button').map((b) => b.getAttribute('aria-label'))).toEqual([
      'Reset password of demo',
      'Reset 2FA of demo',
      'Disable demo',
      'Delete demo',
    ]);
  });

  it('creates a user and shows the temporary password once, with a copy button', async () => {
    const created: NewUserResponse = { user: user({ id: 9, username: 'newbie', totp_enabled: false, must_change_password: true, files: 0, active_tokens: 0, has_data: false }), temporary_password: 'Tmp-pass-1234' };
    const { calls } = open({ 'POST /api/admin/users': () => json(created, { status: 201 }) });
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Create user' }));
    const dialog = await screen.findByRole('dialog', { name: 'Create a user' });
    const submit = within(dialog).getByRole('button', { name: 'Create user' });
    expect(submit).toBeDisabled();
    // The role defaults to a normal user.
    expect(within(dialog).getByLabelText('Role')).toHaveValue('user');
    await u.type(within(dialog).getByLabelText('Username'), 'NewBie');
    await u.click(submit);

    const shown = await screen.findByRole('dialog', { name: 'User created: newbie' });
    expect(within(shown).getByLabelText('Temporary password for newbie')).toHaveValue('Tmp-pass-1234');
    expect(within(shown).getByRole('alert')).toHaveTextContent('shown only once');
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/admin/users' && c.init?.method === 'POST')?.init?.body))).toEqual({ username: 'newbie', role: 'user' });
    await u.click(within(shown).getByRole('button', { name: 'Copy password' }));
    // user-event's clipboard stub took the text.
    expect(await navigator.clipboard.readText()).toBe('Tmp-pass-1234');
    expect(await within(shown).findByText('Copied to the clipboard.')).toBeInTheDocument();

    await u.click(within(shown).getByRole('button', { name: 'I have copied it' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    // Gone for good: the password is nowhere on the page any more.
    expect(screen.queryByDisplayValue('Tmp-pass-1234')).not.toBeInTheDocument();
  });

  it('shows the server\'s refusal when the username is taken', async () => {
    open({ 'POST /api/admin/users': () => json({ error: 'that username is taken' }, { status: 409 }) });
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Create user' }));
    const dialog = await screen.findByRole('dialog', { name: 'Create a user' });
    await u.type(within(dialog).getByLabelText('Username'), 'demo');
    await u.click(within(dialog).getByRole('button', { name: 'Create user' }));
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('that username is taken');
  });

  it('resets a password after a confirmation and shows the new temporary password once', async () => {
    const { calls } = open({ 'POST /api/admin/users/2/reset-password': { temporary_password: 'Fresh-temp-9876' } });
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Reset password of demo' }));
    const confirm = await screen.findByRole('dialog', { name: 'Reset the password of demo?' });
    expect(calls.some((c) => c.url.endsWith('reset-password'))).toBe(false);
    await u.click(within(confirm).getByRole('button', { name: 'Reset password' }));
    const shown = await screen.findByRole('dialog', { name: 'New temporary password for demo' });
    expect(within(shown).getByLabelText('Temporary password for demo')).toHaveValue('Fresh-temp-9876');
    await u.click(within(shown).getByRole('button', { name: 'I have copied it' }));
    expect(screen.queryByDisplayValue('Fresh-temp-9876')).not.toBeInTheDocument();
  });

  it('resets two-factor after a confirmation', async () => {
    const { calls } = open({ 'POST /api/admin/users/2/reset-2fa': () => new Response(null, { status: 204 }) });
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Reset 2FA of demo' }));
    const confirm = await screen.findByRole('dialog', { name: 'Reset two-factor for demo?' });
    await u.click(within(confirm).getByRole('button', { name: 'Reset two-factor' }));
    expect(await screen.findByRole('dialog', { name: 'Two-factor reset' })).toBeInTheDocument();
    expect(calls.some((c) => c.url === '/api/admin/users/2/reset-2fa' && c.init?.method === 'POST')).toBe(true);
  });

  it('disables with a confirmation and enables with one click; a refusal is shown', async () => {
    const { calls } = open(
      { 'POST /api/admin/users/2/disable': () => new Response(null, { status: 204 }), 'POST /api/admin/users/3/enable': () => json({ error: 'nope' }, { status: 400 }) },
      users(admin, user(), user({ id: 3, username: 'bob', disabled: true, has_data: false, files: 0, active_tokens: 0 })),
    );
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Disable demo' }));
    const confirm = await screen.findByRole('dialog', { name: 'Disable demo?' });
    await u.click(within(confirm).getByRole('button', { name: 'Disable user' }));
    await vi.waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(calls.some((c) => c.url === '/api/admin/users/2/disable')).toBe(true);

    await u.click(screen.getByRole('button', { name: 'Enable bob' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('nope');
    expect(calls.some((c) => c.url === '/api/admin/users/3/enable')).toBe(true);
  });

  it('deleting a user without data is one confirmation', async () => {
    const { calls } = open({ 'DELETE /api/admin/users/3': () => new Response(null, { status: 204 }) }, users(admin, user({ id: 3, username: 'bob', has_data: false, files: 0, active_tokens: 0 })));
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Delete bob' }));
    const confirm = await screen.findByRole('dialog', { name: 'Delete bob?' });
    await u.click(within(confirm).getByRole('button', { name: 'Delete user' }));
    await vi.waitFor(() => expect(calls.some((c) => c.url === '/api/admin/users/3' && c.init?.method === 'DELETE')).toBe(true));
  });

  it('deleting a user with data takes two steps and sends confirm=true only at the end', async () => {
    const { calls } = open({ 'DELETE /api/admin/users/2?confirm=true': () => new Response(null, { status: 204 }) });
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Delete demo' }));
    const first = await screen.findByRole('dialog', { name: 'Delete demo?' });
    expect(first).toHaveTextContent('1,284 files and 1 active device token');
    expect(first).toHaveTextContent('original files are never deleted');
    await u.click(within(first).getByRole('button', { name: 'Continue' }));
    expect(calls.some((c) => c.init?.method === 'DELETE')).toBe(false);

    const second = await screen.findByRole('dialog', { name: 'Delete demo and unassign their data?' });
    await u.click(within(second).getByRole('button', { name: 'Back' }));
    await u.click(await screen.findByRole('button', { name: 'Continue' }));
    await u.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Delete and unassign' }));
    await vi.waitFor(() => expect(calls.filter((c) => c.init?.method === 'DELETE').map((c) => c.url)).toEqual(['/api/admin/users/2?confirm=true']));
  });

  it('shows the 409 of the last active admin', async () => {
    open({ 'POST /api/admin/users/5/disable': () => json({ error: 'this is the last active admin' }, { status: 409 }) }, users(admin, user({ id: 5, username: 'other', role: 'admin', has_data: false, files: 0, active_tokens: 0 })));
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Disable other' }));
    await u.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Disable user' }));
    expect(await within(await screen.findByRole('dialog')).findByRole('alert')).toHaveTextContent('last active admin');
  });
});

describe('Admin: unassigned data', () => {
  it('has an empty state when nothing is unassigned', async () => {
    open();
    expect(await screen.findByText('Nothing is unassigned')).toBeInTheDocument();
  });

  it('lists tokens with their file counts, the unattributed files and the legacy records', async () => {
    open({}, users(admin, user()), unassigned());
    const card = await unassignedCard();
    expect(within(card).getByText('Pixel 9a')).toBeInTheDocument();
    expect(within(card).getByText(/312 files/)).toBeInTheDocument();
    expect(within(card).getByText('4 unattributed files')).toBeInTheDocument();
    expect(within(card).getByText('Records from before accounts')).toBeInTheDocument();
    expect(within(card).getByText(/preferences, 3 body entries, 2 edited sets/)).toBeInTheDocument();
    // Only an enabled user can receive data, never an admin.
    const target = within(card).getByLabelText('Give to');
    expect(within(target).getAllByRole('option').map((o) => o.textContent)).toEqual(['Choose a user', 'demo']);
    expect(within(card).getByRole('button', { name: 'Assign' })).toBeDisabled();
  });

  it('assigns the picked tokens, unattributed files and legacy records to the chosen user', async () => {
    const result: AssignResponse = { tokens_moved: 1, files_moved: 316, legacy_adopted: true };
    const { calls } = open({ 'POST /api/admin/unassigned/assign': result }, users(admin, user()), unassigned());
    const u = userEvent.setup();
    const card = await unassignedCard();
    await u.click(within(card).getByRole('checkbox', { name: /Pixel 9a/ }));
    await u.click(within(card).getByRole('checkbox', { name: /unattributed files/ }));
    await u.click(within(card).getByRole('checkbox', { name: /Records from before accounts/ }));
    await u.selectOptions(within(card).getByLabelText('Give to'), 'demo');
    await u.click(within(card).getByRole('button', { name: 'Assign' }));
    expect(await within(card).findByText('Gave 1 device token, 316 files, the records from before accounts to demo.')).toBeInTheDocument();
    const call = calls.find((c) => c.url === '/api/admin/unassigned/assign');
    expect(JSON.parse(String(call?.init?.body))).toEqual({ token_ids: [7], user_id: 2, include_unattributed: true, adopt_legacy: true });
  });

  it('still confirms the handover when it was the last item and the form is gone', async () => {
    // After the POST the list is refetched empty: the form unmounts while the answer is processed.
    let handed = false;
    open(
      {
        'POST /api/admin/unassigned/assign': () => {
          handed = true;
          return json({ tokens_moved: 1, files_moved: 0, legacy_adopted: false });
        },
        'GET /api/admin/unassigned': () => json(handed ? NOTHING : unassigned({ unattributed_files: 0, legacy: NOTHING.legacy })),
      },
      users(admin, user()),
    );
    const u = userEvent.setup();
    const card = await unassignedCard();
    await u.click(within(card).getByRole('checkbox', { name: /Pixel 9a/ }));
    await u.selectOptions(within(card).getByLabelText('Give to'), 'demo');
    await u.click(within(card).getByRole('button', { name: 'Assign' }));
    expect(await screen.findByText('Nothing is unassigned')).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Gave 1 device token, 0 files to demo.');
  });

  it('assigns just a token and shows the server\'s refusal for a conflict', async () => {
    const { calls } = open({ 'POST /api/admin/unassigned/assign': () => json({ error: 'token 7 is not an active unassigned token' }, { status: 409 }) }, users(admin, user()), unassigned({ unattributed_files: 0, legacy: NOTHING.legacy }));
    const u = userEvent.setup();
    const card = await unassignedCard();
    expect(within(card).queryByText(/unattributed/)).not.toBeInTheDocument();
    expect(within(card).queryByText('Records from before accounts')).not.toBeInTheDocument();
    await u.click(within(card).getByRole('checkbox', { name: /Pixel 9a/ }));
    await u.selectOptions(within(card).getByLabelText('Give to'), 'demo');
    await u.click(within(card).getByRole('button', { name: 'Assign' }));
    expect(await within(card).findByRole('alert')).toHaveTextContent('not an active unassigned token');
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/admin/unassigned/assign')?.init?.body))).toEqual({ token_ids: [7], user_id: 2 });
  });

  it('summaries and the legacy check', () => {
    expect(assignSummary({ tokens_moved: 2, files_moved: 1, legacy_adopted: false }, 'demo')).toBe('Gave 2 device tokens, 1 file to demo.');
    expect(hasLegacy(NOTHING.legacy)).toBe(false);
    expect(hasLegacy({ ...NOTHING.legacy, garmin_account: true })).toBe(true);
  });
});
