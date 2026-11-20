import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { passwordProblem, usernameProblem } from '../account/passwords';
import { json, mockApi } from './body/testUtils';
import { SetupPage } from './SetupPage';

function renderSetup() {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}>
      <MemoryRouter initialEntries={['/setup']}>
        <Routes>
          <Route path="/setup" element={<SetupPage />} />
          <Route path="/login" element={<p>login page</p>} />
          <Route path="/admin" element={<p>admin page</p>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const start = { pending_token: 'pending-1', otpauth_uri: 'otpauth://totp/Nimrag%20Connect:boss?secret=JBSWY3DPEHPK3PXP&issuer=Nimrag%20Connect', secret: 'JBSWY3DPEHPK3PXP' };
const admin = { step: 'done', authenticated: true, username: 'boss', role: 'admin', timezone: 'UTC', sample: false };

afterEach(() => vi.unstubAllGlobals());

describe('setup rules', () => {
  it('needs 10 characters and a matching confirmation', () => {
    expect(passwordProblem('short', 'short')).toMatch(/10 characters/);
    expect(passwordProblem('long enough!', 'long enough?')).toMatch(/not the same/);
    expect(passwordProblem('long enough!', 'long enough!')).toBeNull();
  });
  it('checks the username like the server: 3 to 32 of a-z 0-9 . _ -, case-insensitive', () => {
    expect(usernameProblem('ab')).toMatch(/3 to 32/);
    expect(usernameProblem('a'.repeat(33))).toMatch(/3 to 32/);
    expect(usernameProblem('has space')).toMatch(/only/);
    expect(usernameProblem('Boss.One-2_x')).toBeNull();
  });
});

describe('SetupPage', () => {
  it('sends a configured server to the login page', async () => {
    mockApi({ '/api/setup': { needed: false } });
    renderSetup();
    expect(await screen.findByText('login page')).toBeInTheDocument();
  });

  it('collects username and password, shows the QR code and secret, and lands in /admin after the code', async () => {
    const { calls } = mockApi({
      'POST /api/setup/confirm': () => json(admin, { status: 201 }),
      'POST /api/setup': start,
      '/api/setup': { needed: true },
    });
    const user = userEvent.setup();
    renderSetup();

    // The page says what this account is for.
    expect(await screen.findByText(/only sets the server up/)).toBeInTheDocument();
    const next = screen.getByRole('button', { name: 'Continue' });
    expect(next).toBeDisabled();
    await user.type(screen.getByLabelText('Admin username'), 'Boss');
    await user.type(screen.getByLabelText('Password'), 'correct horse');
    await user.type(screen.getByLabelText('Password again'), 'correct horse');
    await user.click(next);

    // Step 2: inline SVG with a text alternative, the secret as text, a one-time-code field.
    const qr = await screen.findByRole('img', { name: /QR code/ });
    expect(qr.tagName.toLowerCase()).toBe('svg');
    expect(qr.querySelector('path')?.getAttribute('d')).toMatch(/^M/);
    expect(screen.getByLabelText('Secret key')).toHaveValue('JBSWY3DPEHPK3PXP');
    const code = screen.getByLabelText('6-digit code');
    expect(code).toHaveAttribute('autocomplete', 'one-time-code');
    expect(code).toHaveAttribute('inputmode', 'numeric');
    expect(code).toHaveAttribute('maxlength', '6');
    // The account is not created yet: only the first request went out.
    expect(calls.filter((c) => c.init?.method === 'POST').map((c) => c.url)).toEqual(['/api/setup']);
    expect(JSON.parse(String(calls.find((c) => c.init?.method === 'POST')?.init?.body))).toEqual({ username: 'boss', password: 'correct horse' });

    const confirm = screen.getByRole('button', { name: 'Create the admin account' });
    expect(confirm).toBeDisabled();
    await user.type(code, '123456');
    await user.click(confirm);
    await waitFor(() => expect(screen.getByText('admin page')).toBeInTheDocument());
    const confirmCall = calls.find((c) => c.url === '/api/setup/confirm');
    expect(JSON.parse(String(confirmCall?.init?.body))).toEqual({ pending_token: 'pending-1', code: '123456' });
  });

  it('a wrong code stays on the step and asks again; an expired step goes back to the first one', async () => {
    let attempt = 0;
    mockApi({
      'POST /api/setup/confirm': () => {
        attempt += 1;
        return attempt === 1 ? json({ error: 'wrong code' }, { status: 401 }) : json({ error: 'too many wrong codes; start the setup again' }, { status: 401 });
      },
      'POST /api/setup': start,
      '/api/setup': { needed: true },
    });
    const user = userEvent.setup();
    renderSetup();
    await user.type(await screen.findByLabelText('Admin username'), 'boss');
    await user.type(screen.getByLabelText('Password'), 'correct horse');
    await user.type(screen.getByLabelText('Password again'), 'correct horse');
    await user.click(screen.getByRole('button', { name: 'Continue' }));

    await user.type(await screen.findByLabelText('6-digit code'), '111111');
    await user.click(screen.getByRole('button', { name: 'Create the admin account' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('That code is not right');
    expect(screen.getByLabelText('6-digit code')).toHaveValue('');

    await user.type(screen.getByLabelText('6-digit code'), '222222');
    await user.click(screen.getByRole('button', { name: 'Create the admin account' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('expired');
    expect(screen.getByLabelText('Admin username')).toHaveValue('boss');
    expect(screen.getByLabelText('Password')).toHaveValue('');
  });

  it('an already configured server (409) goes to the login page', async () => {
    mockApi({
      'POST /api/setup': () => json({ error: 'this server is already set up' }, { status: 409 }),
      '/api/setup': { needed: true },
    });
    const user = userEvent.setup();
    renderSetup();
    await user.type(await screen.findByLabelText('Admin username'), 'boss');
    await user.type(screen.getByLabelText('Password'), 'correct horse');
    await user.type(screen.getByLabelText('Password again'), 'correct horse');
    await user.click(screen.getByRole('button', { name: 'Continue' }));
    expect(await screen.findByText('login page')).toBeInTheDocument();
  });
});
