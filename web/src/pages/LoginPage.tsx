import { useEffect, useRef, useState, type FormEvent } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { api } from '../api/client';
import type { EnrolRequest, LoginRequest, LoginResponse, Me } from '../api/generated';
import { Brand, FormError } from '../account/AuthLayout';
import { classifyStepError, describeLoginError, describeStepError } from '../account/errors';
import { NewPasswordFields } from '../account/PasswordFields';
import { passwordProblem } from '../account/passwords';
import { landingPath, useSetupStatus, useStartSession } from '../account/queries';
import { AuthenticatorPanel, CodeField } from '../account/TotpFields';
import { NativeSessionWait } from '../account/NativeSessionWait';
import { Button, TextField } from '../components';
import { usePageTitle } from '../lib/hooks';
import { hasNativeSession, notifySessionExpired } from '../native';

/** Only same-origin paths are valid redirect targets after login. */
export function safeNext(next: string | null): string {
  if (!next || !next.startsWith('/') || next.startsWith('//') || next.startsWith('/login')) return '/';
  return next;
}

type Enrol = Extract<LoginResponse, { step: 'enrol2fa' }>;
type Change = Extract<LoginResponse, { step: 'change_password' }>;
type Step = { kind: 'credentials' } | { kind: 'enrol'; data: Enrol } | { kind: 'change'; data: Change };

function CredentialsStep({ notice, onNext, onDone }: { notice: string | null; onNext: (step: Step) => void; onDone: (me: Me) => void }) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [code, setCode] = useState('');
  const [error, setError] = useState<string | null>(notice);
  const [busy, setBusy] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const body: LoginRequest = { username: username.trim(), password };
      if (code) body.code = code;
      const res = await api.post<LoginResponse>('/api/login', body, { redirectOn401: false });
      if (res.step === 'enrol2fa') onNext({ kind: 'enrol', data: res });
      else if (res.step === 'change_password') onNext({ kind: 'change', data: res });
      else onDone(res);
    } catch (err) {
      setError(describeLoginError(err));
      setPassword('');
      setCode('');
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="login" onSubmit={(e) => void onSubmit(e)} noValidate aria-label="Log in">
      <Brand sub="Your watch data, on your own server." />
      <TextField
        label="Username"
        name="username"
        autoComplete="username"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        required
        value={username}
        onChange={(e) => setUsername(e.target.value)}
        aria-invalid={error ? true : undefined}
      />
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
      <CodeField value={code} onChange={setCode} hint="From your authenticator app. Leave it empty the first time you log in." invalid={error !== null} />
      <FormError>{error}</FormError>
      <Button type="submit" variant="primary" size="lg" block disabled={busy || username.trim().length === 0 || password.length === 0}>
        {busy ? 'Logging in' : 'Log in'}
      </Button>
    </form>
  );
}

function EnrolStep({ data, onDone, onRestart }: { data: Enrol; onDone: (me: Me) => void; onRestart: (message: string | null) => void }) {
  const [code, setCode] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const codeInput = useRef<HTMLInputElement>(null);
  const passwordMissing = data.must_change_password ? passwordProblem(password, confirm) : null;

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (busy || code.length !== 6 || passwordMissing) return;
    setBusy(true);
    setError(null);
    try {
      const body: EnrolRequest = { pending_token: data.pending_token, code };
      if (data.must_change_password) body.new_password = password;
      const me = await api.post<Me>('/api/login/totp-enrol', body, { redirectOn401: false });
      onDone(me);
    } catch (err) {
      const kind = classifyStepError(err);
      if (kind === 'expired') {
        onRestart('That step expired. Log in again.');
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
      <Brand sub="Set up two-factor to finish logging in." />
      <p className="stat__sub">Your account needs a second step. Add it to an authenticator app, then enter the 6-digit code the app shows.</p>
      <AuthenticatorPanel uri={data.otpauth_uri} secret={data.secret} />
      {data.must_change_password ? (
        <NewPasswordFields password={password} confirm={confirm} onPassword={setPassword} onConfirm={setConfirm} hint="Replaces the temporary password you were given. At least 10 characters." />
      ) : null}
      <CodeField ref={codeInput} value={code} onChange={setCode} />
      <FormError>{error}</FormError>
      <Button type="submit" variant="primary" size="lg" block disabled={busy || code.length !== 6 || passwordMissing !== null}>
        {busy ? 'Checking' : 'Finish and log in'}
      </Button>
      <Button onClick={() => onRestart(null)}>Start over</Button>
    </form>
  );
}

function ChangeStep({ data, onDone, onRestart }: { data: Change; onDone: (me: Me) => void; onRestart: (message: string | null) => void }) {
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
      const me = await api.post<Me>('/api/login/change-password', { pending_token: data.pending_token, new_password: password }, { redirectOn401: false });
      onDone(me);
    } catch (err) {
      if (classifyStepError(err) === 'expired') {
        onRestart('That step expired. Log in again.');
        return;
      }
      setError(describeStepError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="login" onSubmit={(e) => void onSubmit(e)} noValidate aria-label="Choose a new password">
      <Brand sub="Choose your own password." />
      <p className="stat__sub">You logged in with a temporary password. Choose a new one to continue.</p>
      <NewPasswordFields password={password} confirm={confirm} onPassword={setPassword} onConfirm={setConfirm} />
      <FormError>{error}</FormError>
      <Button type="submit" variant="primary" size="lg" block disabled={busy || problem !== null}>
        {busy ? 'Saving' : 'Save password and log in'}
      </Button>
      <Button onClick={() => onRestart(null)}>Start over</Button>
    </form>
  );
}

/**
 * Login: username, password and the 6-digit code in one form (POST /api/login). A person who has
 * no two-factor yet is led through enrolling it, and one with a temporary password through
 * choosing their own, before the session starts.
 */
export function LoginPage() {
  usePageTitle('Log in');
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const startSession = useStartSession();
  const setup = useSetupStatus();
  const [step, setStep] = useState<Step>({ kind: 'credentials' });
  const [notice, setNotice] = useState<string | null>(null);
  // Inside the Android app the app signs in (and reloads); this page is never shown there.
  const nativeTakesOver = hasNativeSession();
  useEffect(() => {
    if (nativeTakesOver) notifySessionExpired();
  }, [nativeTakesOver]);

  // A new server has no account yet: the first visit sets up the admin instead.
  useEffect(() => {
    if (setup.data?.needed) void navigate('/setup', { replace: true });
  }, [setup.data, navigate]);

  function done(me: Me) {
    startSession(me);
    void navigate(landingPath(me, params.get('next')), { replace: true });
  }

  function restart(message: string | null) {
    setNotice(message);
    setStep({ kind: 'credentials' });
  }

  if (nativeTakesOver) return <NativeSessionWait />;

  return (
    <main className="center-page">
      {step.kind === 'enrol' ? (
        <EnrolStep data={step.data} onDone={done} onRestart={restart} />
      ) : step.kind === 'change' ? (
        <ChangeStep data={step.data} onDone={done} onRestart={restart} />
      ) : (
        <CredentialsStep notice={notice} onNext={setStep} onDone={done} />
      )}
    </main>
  );
}
