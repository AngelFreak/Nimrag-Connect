import { useEffect, useRef, useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { ApiError, api } from '../api/client';
import type { Me, SetupConfirmRequest, SetupStartRequest, SetupStartResponse } from '../api/generated';
import { Brand, FormError } from '../account/AuthLayout';
import { classifyStepError, describeStepError } from '../account/errors';
import { NewPasswordFields } from '../account/PasswordFields';
import { MIN_PASSWORD_LEN, USERNAME_MAX, USERNAME_MIN, normaliseUsername, passwordProblem, usernameProblem } from '../account/passwords';
import { landingPath, useSetupStatus, useStartSession } from '../account/queries';
import { AuthenticatorPanel, CodeField } from '../account/TotpFields';
import { Button, TextField } from '../components';
import { usePageTitle } from '../lib/hooks';

function describeStartError(error: unknown): string {
  if (error instanceof ApiError && (error.status === 400 || error.status === 409)) return error.message;
  return describeStepError(error);
}

/** Step 1: the admin's username and password. Nothing is created yet; the server answers with a 2FA secret. */
function AccountStep({ initialUsername, notice, onNext }: { initialUsername: string; notice: string | null; onNext: (username: string, data: SetupStartResponse) => void }) {
  const navigate = useNavigate();
  const [username, setUsername] = useState(initialUsername);
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState<string | null>(notice);
  const [busy, setBusy] = useState(false);
  const nameProblem = username ? usernameProblem(username) : 'Choose a username.';
  const problem = nameProblem ?? passwordProblem(password, confirm);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (busy || problem) return;
    setBusy(true);
    setError(null);
    try {
      const body: SetupStartRequest = { username: normaliseUsername(username), password };
      const data = await api.post<SetupStartResponse>('/api/setup', body, { redirectOn401: false });
      onNext(body.username, data);
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        void navigate('/login', { replace: true });
        return;
      }
      setError(describeStartError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="login" onSubmit={(e) => void onSubmit(e)} noValidate aria-label="Create the admin account">
      <Brand sub="Welcome. Set up this server." />
      <p className="stat__sub">This first account only sets the server up and manages the other accounts. It has no data of its own: people get their own accounts from it.</p>
      <TextField
        label="Admin username"
        name="username"
        autoComplete="username"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        required
        value={username}
        onChange={(e) => setUsername(e.target.value)}
        hint={`${USERNAME_MIN} to ${USERNAME_MAX} characters: a to z, digits, '.', '_' and '-'.`}
        aria-invalid={username && nameProblem ? true : undefined}
      />
      <NewPasswordFields
        label="Password"
        confirmLabel="Password again"
        password={password}
        confirm={confirm}
        onPassword={setPassword}
        onConfirm={setConfirm}
        hint={`At least ${MIN_PASSWORD_LEN} characters. You log in with it, and a code from your authenticator app.`}
      />
      <FormError>{error}</FormError>
      {!error && username && nameProblem ? (
        <p className="stat__sub" role="status">
          {nameProblem}
        </p>
      ) : null}
      <Button type="submit" variant="primary" size="lg" block disabled={busy || problem !== null}>
        {busy ? 'Continuing' : 'Continue'}
      </Button>
    </form>
  );
}

/** Step 2: add the secret to an authenticator app and prove it works with a code. Only now is the admin created. */
function CodeStep({ username, data, onDone, onRestart }: { username: string; data: SetupStartResponse; onDone: (me: Me) => void; onRestart: (message: string | null) => void }) {
  const [code, setCode] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const codeInput = useRef<HTMLInputElement>(null);
  const navigate = useNavigate();

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (busy || code.length !== 6) return;
    setBusy(true);
    setError(null);
    try {
      const body: SetupConfirmRequest = { pending_token: data.pending_token, code };
      const me = await api.post<Me>('/api/setup/confirm', body, { redirectOn401: false });
      onDone(me);
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) {
        void navigate('/login', { replace: true });
        return;
      }
      const kind = classifyStepError(err);
      if (kind === 'expired') {
        onRestart('That step expired. Start again.');
        return;
      }
      setError(describeStepError(err));
      setCode('');
      if (kind === 'wrong-code') codeInput.current?.focus();
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="login" onSubmit={(e) => void onSubmit(e)} noValidate aria-label="Set up two-factor">
      <Brand sub={`Two-factor for ${username}.`} />
      <p className="stat__sub">Add the account to an authenticator app, then enter the 6-digit code it shows. The admin account is created when the code is right.</p>
      <AuthenticatorPanel uri={data.otpauth_uri} secret={data.secret} />
      <CodeField ref={codeInput} value={code} onChange={setCode} />
      <FormError>{error}</FormError>
      <Button type="submit" variant="primary" size="lg" block disabled={busy || code.length !== 6}>
        {busy ? 'Checking' : 'Create the admin account'}
      </Button>
      <Button onClick={() => onRestart(null)}>Start over</Button>
    </form>
  );
}

/** First run: the admin's username and password (POST /api/setup), then 2FA (POST /api/setup/confirm). Lands in /admin. */
export function SetupPage() {
  usePageTitle('Set up');
  const navigate = useNavigate();
  const status = useSetupStatus();
  const startSession = useStartSession();
  // Held in memory between the two steps; the password itself is not: the server keeps its hash.
  const [pending, setPending] = useState<{ username: string; data: SetupStartResponse } | null>(null);
  const [username, setUsername] = useState('');
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (!pending && status.data && !status.data.needed) void navigate('/login', { replace: true });
  }, [pending, status.data, navigate]);

  function done(me: Me) {
    startSession(me);
    void navigate(landingPath(me, null), { replace: true });
  }

  return (
    <div className="center-page">
      {pending ? (
        <CodeStep
          username={pending.username}
          data={pending.data}
          onDone={done}
          onRestart={(message) => {
            setUsername(pending.username);
            setNotice(message);
            setPending(null);
          }}
        />
      ) : (
        <AccountStep
          initialUsername={username}
          notice={notice}
          onNext={(name, data) => {
            setPending({ username: name, data });
            setNotice(null);
          }}
        />
      )}
    </div>
  );
}
