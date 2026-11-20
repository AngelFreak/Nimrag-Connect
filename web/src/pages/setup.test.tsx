import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/client';
import { SetupPage, describeSetupError, passwordProblem } from './SetupPage';

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

function renderSetup() {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter initialEntries={['/setup']}>
        <Routes>
          <Route path="/setup" element={<SetupPage />} />
          <Route path="/login" element={<p>login page</p>} />
          <Route path="/" element={<p>home page</p>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.unstubAllGlobals());

describe('setup rules', () => {
  it('needs 10 characters and a matching confirmation', () => {
    expect(passwordProblem('short', 'short')).toMatch(/10 characters/);
    expect(passwordProblem('long enough!', 'long enough?')).toMatch(/not the same/);
    expect(passwordProblem('long enough!', 'long enough!')).toBeNull();
  });
  it('explains an already configured server', () => {
    expect(describeSetupError(new ApiError(409, 'x'))).toMatch(/Log in instead/);
  });
});

describe('SetupPage', () => {
  it('sends a configured server to the login page', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => json(200, { needed: false })),
    );
    renderSetup();
    expect(await screen.findByText('login page')).toBeInTheDocument();
  });

  it('saves the password, then creates the phone token with it', async () => {
    const calls: { url: string; body: unknown }[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, init?: RequestInit) => {
        calls.push({
          url,
          body: init?.body ? JSON.parse(String(init.body)) : null,
        });
        if (url === '/api/setup' && init?.method === 'POST')
          return json(201, {
            authenticated: true,
            timezone: 'UTC',
            sample: false,
          });
        if (url === '/api/setup') return json(200, { needed: true });
        if (url === '/api/tokens') return json(201, { id: 1, name: 'Pixel 9a', token: 'secret-token' });
        return json(404, { error: 'no' });
      }),
    );
    const user = userEvent.setup();
    renderSetup();
    await user.type(screen.getByLabelText('Password'), 'correct horse');
    await user.type(screen.getByLabelText('Password again'), 'correct horse');
    await user.click(screen.getByRole('button', { name: 'Save password' }));
    expect(await screen.findByRole('heading', { name: 'Connect your phone' })).toBeInTheDocument();
    expect(screen.getByLabelText('Server address')).toHaveValue(window.location.origin);

    const name = screen.getByLabelText('Device name');
    await user.clear(name);
    await user.type(name, 'Pixel 9a');
    await user.click(screen.getByRole('button', { name: 'Create device token' }));
    expect(await screen.findByDisplayValue('secret-token')).toBeInTheDocument();
    expect(calls.find((c) => c.url === '/api/tokens')?.body).toEqual({
      name: 'Pixel 9a',
      password: 'correct horse',
    });

    await user.click(screen.getByRole('button', { name: 'I have saved it' }));
    await waitFor(() => expect(screen.getByText('home page')).toBeInTheDocument());
  });
});
