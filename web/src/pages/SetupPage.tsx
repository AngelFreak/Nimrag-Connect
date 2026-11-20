import { useEffect, useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { ApiError, NetworkError, api } from '../api/client';
import type { Me, NewTokenResponse, SetupStatus } from '../api/generated';
import { Button, TextField } from '../components';
import { AlertIcon } from '../components/icons';
import { usePageTitle } from '../lib/hooks';
import { NewTokenNotice, describeTokenError } from './system/TokensCard';
import { useCreateToken } from './system/queries';

/** Same rule as the server (`MIN_PASSWORD_LEN`). */
export const MIN_PASSWORD_LEN = 10;

export const SETUP_KEY = ['setup'] as const;

/** GET /api/setup: whether the server still waits for its owner password. Needs no session. */
export function useSetupStatus() {
  return useQuery({
    queryKey: SETUP_KEY,
    queryFn: ({ signal }) => api.get<SetupStatus>('/api/setup', { signal }),
    staleTime: 0,
  });
}

/** Why the password form cannot be sent yet, or null when it can. */
export function passwordProblem(password: string, confirm: string): string | null {
  if ([...password].length < MIN_PASSWORD_LEN) return `Use at least ${MIN_PASSWORD_LEN} characters.`;
  if (password !== confirm) return 'The two passwords are not the same.';
  return null;
}

export function describeSetupError(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 409) return 'This server already has a password. Log in instead.';
    if (error.status === 400) return error.message;
    return `The server answered with an error (${error.status}). Try again.`;
  }
  if (error instanceof NetworkError) return 'Could not reach the server. Check your connection and try again.';
  return 'Something went wrong. Try again.';
}

function Brand({ sub }: { sub: string }) {
  return (
    <div className="login__brand">
      <img src="/icon.svg" alt="" width={44} height={44} />
      <div>
        <h1 className="login__title">Nimrag Connect</h1>
        <p className="login__sub">{sub}</p>
      </div>
    </div>
  );
}

function PasswordStep({ onDone }: { onDone: (password: string) => void }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const problem = passwordProblem(password, confirm);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (busy || problem) return;
    setBusy(true);
    setError(null);
    try {
      const me = await api.post<Me>('/api/setup', { password }, { redirectOn401: false });
      queryClient.clear();
      queryClient.setQueryData(['me'], me);
      onDone(password);
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        void navigate('/login', { replace: true });
        return;
      }
      setError(describeSetupError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="login" onSubmit={(e) => void onSubmit(e)} noValidate aria-label="Choose a password">
      <Brand sub="Welcome. Choose the password for this server." />
      <TextField
        label="Password"
        type="password"
        name="new-password"
        autoComplete="new-password"
        required
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        hint={`At least ${MIN_PASSWORD_LEN} characters. You log in with it in the browser and in the app.`}
      />
      <TextField
        label="Password again"
        type="password"
        name="confirm-password"
        autoComplete="new-password"
        required
        value={confirm}
        onChange={(e) => setConfirm(e.target.value)}
        aria-invalid={confirm && password !== confirm ? true : undefined}
      />
      {error ? (
        <p className="login__error" role="alert">
          <AlertIcon size={20} />
          <span>{error}</span>
        </p>
      ) : confirm && problem ? (
        <p className="stat__sub" role="status">
          {problem}
        </p>
      ) : null}
      <Button type="submit" variant="primary" size="lg" block disabled={busy || problem !== null}>
        {busy ? 'Saving' : 'Save password'}
      </Button>
    </form>
  );
}

function PhoneStep({ password, onFinish }: { password: string; onFinish: () => void }) {
  const create = useCreateToken();
  const [name, setName] = useState('Phone');
  const [token, setToken] = useState<NewTokenResponse | null>(null);
  const server = window.location.origin;

  function submit(e: FormEvent) {
    e.preventDefault();
    create.mutate({ name: name.trim(), password }, { onSuccess: setToken });
  }

  return (
    <div className="login" role="group" aria-labelledby="setup-phone-title">
      <Brand sub="Password saved. You are logged in." />
      <h2 className="token-secret__title" id="setup-phone-title">
        Connect your phone
      </h2>
      <p className="stat__sub">The Nimrag app uploads your watch files with a device token. In the app, open Watch, then Settings, and enter the server address and the token.</p>
      <label className="field__label" htmlFor="setup-server">
        Server address
      </label>
      <input id="setup-server" className="field__input" readOnly value={server} onFocus={(e) => e.currentTarget.select()} />
      {token ? (
        <NewTokenNotice token={token} onClose={onFinish} />
      ) : (
        <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: 12 }} aria-label="Create a device token">
          <TextField label="Device name" maxLength={64} value={name} onChange={(e) => setName(e.target.value)} hint='For example "Pixel 9a".' required />
          <Button type="submit" variant="primary" size="lg" block disabled={create.isPending || !name.trim()} aria-busy={create.isPending}>
            {create.isPending ? 'Creating' : 'Create device token'}
          </Button>
          {create.isError ? (
            <p className="status-msg status-msg--error" role="alert">
              {describeTokenError(create.error)}
            </p>
          ) : null}
          <Button onClick={onFinish}>Skip for now</Button>
          <p className="stat__sub">You can create tokens later in Settings, under Device tokens.</p>
        </form>
      )}
    </div>
  );
}

/** First run: choose the owner password (POST /api/setup), then create the phone's device token. */
export function SetupPage() {
  usePageTitle('Set up');
  const navigate = useNavigate();
  const status = useSetupStatus();
  // The password is held in memory only between the two steps, so the token request needs
  // no second prompt; it is dropped when setup finishes.
  const [phone, setPhone] = useState<{ password: string } | null>(null);

  useEffect(() => {
    if (!phone && status.data && !status.data.needed) void navigate('/login', { replace: true });
  }, [phone, status.data, navigate]);

  // Leaving the page unmounts it, which drops the password with it.
  function finish() {
    void navigate('/', { replace: true });
  }

  return <div className="center-page">{phone ? <PhoneStep password={phone.password} onFinish={finish} /> : <PasswordStep onDone={(password) => setPhone({ password })} />}</div>;
}
