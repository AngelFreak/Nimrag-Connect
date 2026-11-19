import { useState, type FormEvent } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { ApiError, NetworkError, api } from '../api/client';
import { Button, TextField } from '../components';
import { AlertIcon } from '../components/icons';
import { usePageTitle } from '../lib/hooks';

/** Only same-origin paths are valid redirect targets after login. */
export function safeNext(next: string | null): string {
  if (!next || !next.startsWith('/') || next.startsWith('//') || next.startsWith('/login')) return '/';
  return next;
}

export function describeLoginError(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 401) return 'Wrong password.';
    if (error.status === 429) return 'Too many attempts. Wait a little and try again.';
    return `The server answered with an error (${error.status}). Try again.`;
  }
  if (error instanceof NetworkError) return 'Could not reach the server. Check your connection and try again.';
  return 'Something went wrong. Try again.';
}

/** Password login: POST /api/login, then GET /api/me to confirm the session. */
export function LoginPage() {
  usePageTitle('Log in');
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const queryClient = useQueryClient();
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.post('/api/login', { password }, { redirectOn401: false });
      const me = await api.get('/api/me');
      queryClient.clear();
      queryClient.setQueryData(['me'], me);
      void navigate(safeNext(params.get('next')), { replace: true });
    } catch (err) {
      setError(describeLoginError(err));
      setPassword('');
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="center-page">
      <form className="login" onSubmit={(e) => void onSubmit(e)} noValidate>
        <div className="login__brand">
          <img src="/icon.svg" alt="" width={44} height={44} />
          <div>
            <h1 className="login__title">Nimrag Connect</h1>
            <p className="login__sub">Your watch data, on your own server.</p>
          </div>
        </div>
        <TextField
          label="Password"
          type="password"
          name="password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          aria-invalid={error ? true : undefined}
        />
        {error ? (
          <p className="login__error" role="alert">
            <AlertIcon size={20} />
            <span>{error}</span>
          </p>
        ) : null}
        <Button type="submit" variant="primary" size="lg" block disabled={busy || password.length === 0}>
          {busy ? 'Logging in' : 'Log in'}
        </Button>
      </form>
    </div>
  );
}
