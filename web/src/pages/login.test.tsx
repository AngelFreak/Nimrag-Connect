import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ME_KEY, landingPath } from '../account/queries';
import { json, mockApi } from './body/testUtils';
import { LoginPage } from './LoginPage';

function Where() {
  const l = useLocation();
  return <p>{`at ${l.pathname}`}</p>;
}

function renderLogin(path = '/login') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/login" element={<LoginPage />} />
          <Route path="/setup" element={<p>setup page</p>} />
          <Route path="*" element={<Where />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return client;
}

const me = (over: Record<string, unknown> = {}) => ({ authenticated: true, username: 'demo', role: 'user', timezone: 'Europe/Copenhagen', sample: false, ...over });
const enrol = (must_change_password = false) => ({
  step: 'enrol2fa',
  pending_token: 'pending-enrol',
  otpauth_uri: 'otpauth://totp/Nimrag%20Connect:newbie?secret=KRSXG5CTMVRXEZLU&issuer=Nimrag%20Connect',
  secret: 'KRSXG5CTMVRXEZLU',
  must_change_password,
});

async function fillCredentials(user: ReturnType<typeof userEvent.setup>, code = '') {
  await user.type(await screen.findByLabelText('Username'), 'demo');
  await user.type(screen.getByLabelText('Password'), 'correct horse');
  if (code) await user.type(screen.getByLabelText('6-digit code'), code);
}

afterEach(() => vi.unstubAllGlobals());

describe('landingPath', () => {
  it('sends an admin to /admin and a user to where they were going', () => {
    expect(landingPath(me({ role: 'admin' }) as never, '/activities')).toBe('/admin');
    expect(landingPath(me({ role: 'admin' }) as never, '/admin')).toBe('/admin');
    expect(landingPath(me() as never, '/activities?type=run')).toBe('/activities?type=run');
    expect(landingPath(me() as never, '/admin')).toBe('/');
    expect(landingPath(me() as never, '//evil.example')).toBe('/');
    expect(landingPath(me() as never, null)).toBe('/');
  });
});

describe('LoginPage', () => {
  it('has username, password and a one-time-code field in one form', async () => {
    mockApi({ '/api/setup': { needed: false } });
    renderLogin();
    expect(await screen.findByLabelText('Username')).toHaveAttribute('autocomplete', 'username');
    expect(screen.getByLabelText('Password')).toHaveAttribute('autocomplete', 'current-password');
    const code = screen.getByLabelText('6-digit code');
    expect(code).toHaveAttribute('autocomplete', 'one-time-code');
    expect(code).toHaveAttribute('inputmode', 'numeric');
    expect(code).toHaveAttribute('maxlength', '6');
    expect(screen.getByRole('button', { name: 'Log in' })).toBeDisabled();
  });

  it('sends a server without accounts to the setup page', async () => {
    mockApi({ '/api/setup': { needed: true } });
    renderLogin();
    expect(await screen.findByText('setup page')).toBeInTheDocument();
  });

  it('logs in with username, password and code, then goes where the person was heading', async () => {
    const { calls } = mockApi({ 'POST /api/login': () => json({ step: 'done', ...me() }), '/api/setup': { needed: false } });
    const user = userEvent.setup();
    const client = renderLogin('/login?next=%2Factivities');
    await fillCredentials(user, '123456');
    await user.click(screen.getByRole('button', { name: 'Log in' }));
    expect(await screen.findByText('at /activities')).toBeInTheDocument();
    const login = calls.find((c) => c.url === '/api/login');
    expect(JSON.parse(String(login?.init?.body))).toEqual({ username: 'demo', password: 'correct horse', code: '123456' });
    expect((login?.init?.headers as Record<string, string>)['X-Nimrag']).toBe('1');
    // The session's Me is cached without the step tag; no second request is needed.
    expect(client.getQueryData(ME_KEY)).toEqual(me());
  });

  it('sends an admin to /admin whatever the next page was', async () => {
    mockApi({ 'POST /api/login': () => json({ step: 'done', ...me({ username: 'admin', role: 'admin' }) }), '/api/setup': { needed: false } });
    const user = userEvent.setup();
    renderLogin('/login?next=%2Factivities');
    await fillCredentials(user, '123456');
    await user.click(screen.getByRole('button', { name: 'Log in' }));
    expect(await screen.findByText('at /admin')).toBeInTheDocument();
  });

  it('shows one generic message for a wrong username, password or code', async () => {
    mockApi({ 'POST /api/login': () => json({ error: 'wrong username, password or code' }, { status: 401 }), '/api/setup': { needed: false } });
    const user = userEvent.setup();
    renderLogin();
    await fillCredentials(user, '000000');
    await user.click(screen.getByRole('button', { name: 'Log in' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Wrong username, password or code.');
    // It does not reveal which part was wrong, and the secrets are cleared.
    expect(screen.getByLabelText('Password')).toHaveValue('');
    expect(screen.getByLabelText('6-digit code')).toHaveValue('');
    expect(screen.getByLabelText('Username')).toHaveValue('demo');
  });

  it('asks the person to wait when the server rate limits', async () => {
    mockApi({ 'POST /api/login': () => json({ error: 'too many attempts' }, { status: 429, headers: { 'content-type': 'application/json', 'retry-after': '30' } }), '/api/setup': { needed: false } });
    const user = userEvent.setup();
    renderLogin();
    await fillCredentials(user);
    await user.click(screen.getByRole('button', { name: 'Log in' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Too many attempts. Wait a little and try again.');
  });

  it('enrol step: QR code, secret and a code; the session starts when the code is right', async () => {
    const { calls } = mockApi({
      'POST /api/login/totp-enrol': () => json({ step: 'done', ...me({ username: 'newbie' }) }),
      'POST /api/login': () => json(enrol()),
      '/api/setup': { needed: false },
    });
    const user = userEvent.setup();
    renderLogin();
    await fillCredentials(user);
    await user.click(screen.getByRole('button', { name: 'Log in' }));

    expect(await screen.findByRole('img', { name: /QR code/ })).toBeInTheDocument();
    expect(screen.getByLabelText('Secret key')).toHaveValue('KRSXG5CTMVRXEZLU');
    // No new-password fields: the password is not a temporary one.
    expect(screen.queryByLabelText('New password')).not.toBeInTheDocument();
    const finish = screen.getByRole('button', { name: 'Finish and log in' });
    expect(finish).toBeDisabled();
    await user.type(screen.getByLabelText('6-digit code'), '654321');
    await user.click(finish);
    expect(await screen.findByText('at /')).toBeInTheDocument();
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/login/totp-enrol')?.init?.body))).toEqual({ pending_token: 'pending-enrol', code: '654321' });
  });

  it('enrol step with a temporary password also asks for the new password', async () => {
    const { calls } = mockApi({
      'POST /api/login/totp-enrol': () => json({ step: 'done', ...me({ username: 'newbie' }) }),
      'POST /api/login': () => json(enrol(true)),
      '/api/setup': { needed: false },
    });
    const user = userEvent.setup();
    renderLogin();
    await fillCredentials(user);
    await user.click(screen.getByRole('button', { name: 'Log in' }));

    await user.type(await screen.findByLabelText('6-digit code'), '654321');
    const finish = screen.getByRole('button', { name: 'Finish and log in' });
    expect(finish).toBeDisabled();
    await user.type(screen.getByLabelText('New password'), 'a brand new password');
    await user.type(screen.getByLabelText('New password again'), 'a different one!!');
    expect(await screen.findByText('The two passwords are not the same.')).toBeInTheDocument();
    expect(finish).toBeDisabled();
    await user.clear(screen.getByLabelText('New password again'));
    await user.type(screen.getByLabelText('New password again'), 'a brand new password');
    await user.click(finish);
    expect(await screen.findByText('at /')).toBeInTheDocument();
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/login/totp-enrol')?.init?.body))).toEqual({
      pending_token: 'pending-enrol',
      code: '654321',
      new_password: 'a brand new password',
    });
  });

  it('enrol step: a wrong code stays and asks again; an expired step starts over', async () => {
    let attempt = 0;
    mockApi({
      'POST /api/login/totp-enrol': () => {
        attempt += 1;
        return json({ error: attempt === 1 ? 'wrong code' : 'this step has expired; log in again' }, { status: 401 });
      },
      'POST /api/login': () => json(enrol()),
      '/api/setup': { needed: false },
    });
    const user = userEvent.setup();
    renderLogin();
    await fillCredentials(user);
    await user.click(screen.getByRole('button', { name: 'Log in' }));
    await user.type(await screen.findByLabelText('6-digit code'), '111111');
    await user.click(screen.getByRole('button', { name: 'Finish and log in' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('That code is not right');
    expect(screen.getByRole('img', { name: /QR code/ })).toBeInTheDocument();

    await user.type(screen.getByLabelText('6-digit code'), '222222');
    await user.click(screen.getByRole('button', { name: 'Finish and log in' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('That step expired. Log in again.');
    expect(screen.getByLabelText('Username')).toBeInTheDocument();
  });

  it('change-password step: a temporary password must be replaced before the session starts', async () => {
    const { calls } = mockApi({
      'POST /api/login/change-password': () => json({ step: 'done', ...me() }),
      'POST /api/login': () => json({ step: 'change_password', pending_token: 'pending-change' }),
      '/api/setup': { needed: false },
    });
    const user = userEvent.setup();
    renderLogin();
    await fillCredentials(user, '123456');
    await user.click(screen.getByRole('button', { name: 'Log in' }));

    expect(await screen.findByRole('heading', { name: 'Nimrag Connect' })).toBeInTheDocument();
    expect(screen.getByText(/temporary password/)).toBeInTheDocument();
    const save = screen.getByRole('button', { name: 'Save password and log in' });
    expect(save).toBeDisabled();
    await user.type(screen.getByLabelText('New password'), 'too short');
    await user.type(screen.getByLabelText('New password again'), 'too short');
    expect(save).toBeDisabled();
    await user.clear(screen.getByLabelText('New password'));
    await user.clear(screen.getByLabelText('New password again'));
    await user.type(screen.getByLabelText('New password'), 'long enough now');
    await user.type(screen.getByLabelText('New password again'), 'long enough now');
    await user.click(save);
    await waitFor(() => expect(screen.getByText('at /')).toBeInTheDocument());
    expect(JSON.parse(String(calls.find((c) => c.url === '/api/login/change-password')?.init?.body))).toEqual({ pending_token: 'pending-change', new_password: 'long enough now' });
  });

  it('change-password step shows the server\'s own message for a refused password', async () => {
    mockApi({
      'POST /api/login/change-password': () => json({ error: 'choose a password different from the temporary one' }, { status: 400 }),
      'POST /api/login': () => json({ step: 'change_password', pending_token: 'pending-change' }),
      '/api/setup': { needed: false },
    });
    const user = userEvent.setup();
    renderLogin();
    await fillCredentials(user, '123456');
    await user.click(screen.getByRole('button', { name: 'Log in' }));
    await user.type(await screen.findByLabelText('New password'), 'long enough now');
    await user.type(screen.getByLabelText('New password again'), 'long enough now');
    await user.click(screen.getByRole('button', { name: 'Save password and log in' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('choose a password different from the temporary one');
  });
});
