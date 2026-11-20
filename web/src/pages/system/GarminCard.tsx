import { useId, useRef, useState, type ChangeEvent, type FormEvent } from 'react';
import type { GarminImportTokens, GarminStatus } from '../../api/generated';
import type { GarminState } from '../../api/generated/GarminState';
import { Button, Card, CardHeader, Chip, DataList, ErrorState, Notice, Skeleton, TextField } from '../../components';
import { fmtInt } from '../../lib/format';
import { fmtDateTime } from '../today/format';
import { errorMessage } from './errors';
import { useGarmin, useGarminActions } from './queries';

/** What the status chip says. A lost login and a paused queue read the same: the way out is to log in again. */
export const GARMIN_STATE_LABEL: Record<GarminState, string> = {
  off: 'Off',
  needs_login: 'Log in to Garmin again',
  needs_mfa: 'Waiting for the verification code',
  connected: 'Connected',
  paused: 'Log in to Garmin again',
};

/** Reads a python-garminconnect token file (`garmin_tokens.json`). Returns null when it is not one. */
export function parseTokenFile(text: string): GarminImportTokens | null {
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    return null;
  }
  if (!data || typeof data !== 'object') return null;
  const o = data as Record<string, unknown>;
  const token = o['di_token'];
  const refresh = o['di_refresh_token'];
  if (typeof token !== 'string' || typeof refresh !== 'string' || !token || !refresh) return null;
  const clientId = o['di_client_id'];
  return { di_token: token, di_refresh_token: refresh, di_client_id: typeof clientId === 'string' && clientId ? clientId : null };
}

function Failure({ error }: { error: unknown }) {
  return (
    <p className="status-msg status-msg--error" role="alert">
      {errorMessage(error)}
    </p>
  );
}

function LoginForm({ again, onLoggedIn }: { again: boolean; onLoggedIn: () => void }) {
  const { login } = useGarminActions();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');

  function submit(e: FormEvent) {
    e.preventDefault();
    login.mutate(
      { email: email.trim(), password },
      {
        onSuccess: onLoggedIn,
        // The password is used once: clear it whatever the answer.
        onSettled: () => setPassword(''),
      },
    );
  }

  return (
    <form onSubmit={submit} className="form" aria-label="Log in to Garmin">
      <TextField label="Garmin email" type="email" autoComplete="off" value={email} onChange={(e) => setEmail(e.target.value)} required />
      <TextField
        label="Garmin password"
        type="password"
        autoComplete="off"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        required
        hint="Used once to log in, then forgotten."
      />
      <Button type="submit" variant="primary" size="lg" disabled={login.isPending || !email.trim() || !password} aria-busy={login.isPending}>
        {login.isPending ? 'Logging in' : again ? 'Log in to Garmin again' : 'Log in to Garmin'}
      </Button>
      {login.isError ? <Failure error={login.error} /> : null}
    </form>
  );
}

function MfaForm({ onRestart }: { onRestart: () => void }) {
  const { mfa } = useGarminActions();
  const [code, setCode] = useState('');
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        mfa.mutate({ code: code.trim() }, { onSettled: () => setCode('') });
      }}
      className="form"
      aria-label="Garmin verification code"
    >
      <TextField
        label="Verification code"
        inputMode="numeric"
        autoComplete="one-time-code"
        value={code}
        onChange={(e) => setCode(e.target.value)}
        required
        hint="Garmin sent a code to your email or phone. It is good for 10 minutes."
      />
      <div className="form__actions">
        <Button type="submit" variant="primary" size="lg" disabled={mfa.isPending || !code.trim()} aria-busy={mfa.isPending}>
          {mfa.isPending ? 'Checking' : 'Send code'}
        </Button>
        <Button type="button" size="lg" onClick={onRestart}>
          Start over
        </Button>
      </div>
      {mfa.isError ? <Failure error={mfa.error} /> : null}
    </form>
  );
}

function ImportTokens() {
  const { importTokens } = useGarminActions();
  const input = useRef<HTMLInputElement>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const hintId = useId();

  async function pick(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    e.target.value = '';
    setProblem(null);
    importTokens.reset();
    if (!file) return;
    if (file.size > 64 * 1024) {
      setProblem('That file is too big to be a token file.');
      return;
    }
    const tokens = parseTokenFile(await file.text());
    if (!tokens) {
      setProblem('That is not a python-garminconnect token file (garmin_tokens.json).');
      return;
    }
    importTokens.mutate(tokens);
  }

  return (
    <details className="garmin-import">
      <summary>Use a token file instead</summary>
      <div className="form garmin-import__body">
        <p className="card__intro" id={hintId}>
          If Garmin refuses the login from this server, log in with python-garminconnect on another computer and pick the <code>garmin_tokens.json</code> it
          writes. The file goes to your server once and is not shown here.
        </p>
        <input
          ref={input}
          type="file"
          accept="application/json,.json"
          className="sr-only"
          tabIndex={-1}
          aria-label="Garmin token file"
          aria-describedby={hintId}
          onChange={(e) => void pick(e)}
        />
        <Button type="button" disabled={importTokens.isPending} onClick={() => input.current?.click()} aria-busy={importTokens.isPending}>
          {importTokens.isPending ? 'Checking the tokens' : 'Choose token file'}
        </Button>
        {problem ? (
          <p className="status-msg status-msg--error" role="alert">
            {problem}
          </p>
        ) : null}
        {importTokens.isError ? <Failure error={importTokens.error} /> : null}
      </div>
    </details>
  );
}

function Counts({ s, timezone }: { s: GarminStatus; timezone: string }) {
  return (
    <DataList
      items={[
        { label: 'Waiting to send', value: fmtInt(s.pending) },
        { label: 'Sent or already there', value: fmtInt(s.done) },
        { label: 'Needs you', value: fmtInt(s.failed), ...(s.failed > 0 ? { tone: 'warn' as const } : {}) },
        { label: 'Next try', value: s.pending > 0 ? fmtDateTime(s.next_attempt_at, timezone) : 'Nothing waiting' },
      ]}
    />
  );
}

function ConnectedTools({ status, timezone }: { status: GarminStatus; timezone: string }) {
  const { retry, backfill, logout } = useGarminActions();
  const [days, setDays] = useState('30');
  const [confirmLogout, setConfirmLogout] = useState(false);
  const n = Number(days);
  const daysOk = Number.isInteger(n) && n >= 1 && n <= 3650;

  return (
    <>
      <Counts s={status} timezone={timezone} />
      <div className="form__group" style={{ gap: 6 }}>
        <Button disabled={retry.isPending || status.failed === 0} onClick={() => retry.mutate()} aria-busy={retry.isPending}>
          Try the failed ones again
        </Button>
        {retry.isError ? <Failure error={retry.error} /> : null}
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (daysOk) backfill.mutate({ days: n });
        }}
        className="form__group"
        aria-label="Send older activities"
      >
        <TextField
          label="Also send activities from the last (days)"
          type="number"
          inputMode="numeric"
          min={1}
          max={3650}
          value={days}
          onChange={(e) => {
            setDays(e.target.value);
            backfill.reset();
          }}
          aria-invalid={days !== '' && !daysOk ? true : undefined}
          hint={days !== '' && !daysOk ? 'Enter a whole number of days from 1 to 3650.' : 'New activities are sent on their own. This queues the ones you already have.'}
        />
        <Button type="submit" disabled={!daysOk || backfill.isPending} aria-busy={backfill.isPending}>
          {backfill.isPending ? 'Queuing' : 'Send older activities'}
        </Button>
        {backfill.isSuccess ? (
          <p className="status-msg status-msg--ok" role="status">
            {backfill.data.queued === 0
              ? 'Nothing new to queue: those activities are already queued or sent.'
              : `Queued ${fmtInt(backfill.data.queued)} ${backfill.data.queued === 1 ? 'file' : 'files'}.`}
          </p>
        ) : null}
        {backfill.isError ? <Failure error={backfill.error} /> : null}
      </form>
      {confirmLogout ? (
        <div className="form__group" style={{ gap: 10 }}>
          <p className="card__intro">Stops sending activities to Garmin and deletes the stored login tokens from your server.</p>
          <div className="form__actions">
            <Button onClick={() => logout.mutate(undefined, { onSuccess: () => setConfirmLogout(false) })} disabled={logout.isPending}>
              Yes, log out of Garmin
            </Button>
            <Button onClick={() => setConfirmLogout(false)}>Cancel</Button>
          </div>
          {logout.isError ? <Failure error={logout.error} /> : null}
        </div>
      ) : (
        <Button onClick={() => setConfirmLogout(true)}>Log out of Garmin</Button>
      )}
    </>
  );
}

/** Settings, "Garmin Connect (Forward)": the server sends activity files to the owner's Garmin Connect account. */
export function GarminCard({ timezone }: { timezone: string }) {
  const query = useGarmin();
  const [restart, setRestart] = useState(false);
  const s = query.data;
  const state: GarminState | null = s?.state ?? null;
  const label = state ? GARMIN_STATE_LABEL[state] : null;
  const showLogin = state === 'off' || state === 'needs_login' || state === 'paused' || (state === 'needs_mfa' && restart);

  return (
    <Card padding="lg" aria-labelledby="garmin-title">
      <CardHeader title="Garmin Connect (Forward)" id="garmin-title" level={3} aside={label ? <Chip>{label}</Chip> : undefined} />
      <p className="card__intro">
        Sends each activity your watch records on to your Garmin Connect account. Activities only: Garmin&apos;s import takes no sleep, HRV or other
        health data.
      </p>
      <Notice tone="warn">
        The login is unofficial: it copies Garmin&apos;s phone app, may stop working without notice and may conflict with Garmin&apos;s terms. Your password is
        used once and never stored; only login tokens are kept, on your server.
      </Notice>
      {query.isPending ? <Skeleton height={20} lines={3} /> : null}
      {query.isError && !s ? (
        <ErrorState title="Could not load the Garmin status" action={<Button onClick={() => void query.refetch()}>Try again</Button>}>
          {errorMessage(query.error)}
        </ErrorState>
      ) : null}
      {s && state ? (
        <>
          {state === 'connected' ? <ConnectedTools status={s} timezone={timezone} /> : null}
          {state === 'needs_mfa' && !restart ? <MfaForm onRestart={() => setRestart(true)} /> : null}
          {state === 'paused' ? <p className="card__intro">Garmin kept refusing the stored login, so sending is paused. Log in again to carry on.</p> : null}
          {showLogin ? <LoginForm again={state !== 'off'} onLoggedIn={() => setRestart(false)} /> : null}
          {state !== 'connected' ? <ImportTokens /> : null}
          {s.last_error ? (
            <p className="status-msg status-msg--error" role="status">
              Last error: {s.last_error}
            </p>
          ) : null}
        </>
      ) : null}
    </Card>
  );
}
