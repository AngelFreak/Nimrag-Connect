import { useId, useRef, useState, type ChangeEvent, type FormEvent } from 'react';
import type { GarminImportTokens, GarminStatus } from '../../api/generated';
import type { GarminState } from '../../api/generated/GarminState';
import { Button, Card, ConfirmDialog, DataList, DownloadIcon, ErrorState, Notice, SegmentedControl, SendIcon, SettingsGroup, SettingsRow, Skeleton, StatusBanner, TextField } from '../../components';
import { fmtInt } from '../../lib/format';
import { fmtDateTime, fmtLongDay } from '../today/format';
import { useSettings } from '../today/queries';
import { errorMessage } from '../system/errors';
import { useGarmin, useGarminActions } from '../system/queries';
import { SubPage } from './common';

/** The one-line value of the "Send to Garmin" row. A lost login and a paused queue read the same: the way out is to log in again. */
export const GARMIN_STATE_VALUE: Record<GarminState, string> = {
  off: 'Off',
  needs_login: 'Log in again',
  needs_mfa: 'Waiting for the code',
  connected: 'On',
  paused: 'Paused',
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

const SEND_CHOICES = [
  { value: 'activities', label: 'Activities only' },
  { value: 'everything', label: 'Everything from the watch' },
] as const;

/** `YYYY-MM-DD` of `ms` in `timeZone` (what the date field and the server's `since` use). */
export function isoDay(ms: number, timeZone: string): string {
  return new Intl.DateTimeFormat('en-CA', { timeZone, year: 'numeric', month: '2-digit', day: '2-digit' }).format(new Date(ms));
}

/** The choice between activities only and every file the watch makes. Changing it queues nothing. */
function WhatToSend({ status }: { status: GarminStatus }) {
  const { chooseWhatToSend } = useGarminActions();
  const value = status.send_all ? 'everything' : 'activities';
  return (
    <div className="form__group">
      <SegmentedControl
        label="What to send"
        options={SEND_CHOICES}
        value={value}
        onChange={(next) => {
          if (next !== value && !chooseWhatToSend.isPending) chooseWhatToSend.mutate({ send_all: next === 'everything' });
        }}
      />
      <p className="card__intro">
        <strong>Activities only:</strong> each activity your watch records. Garmin Connect gets no health or sleep data from this server.
      </p>
      <p className="card__intro">
        <strong>Everything:</strong> health, sleep and all-day data too, so Garmin Connect stays complete.
      </p>
      {chooseWhatToSend.isError ? <Failure error={chooseWhatToSend.error} /> : null}
    </div>
  );
}

/** The switch for fetching the nights the watch never delivered (the Garmin Connect app synced first). */
function FillNights({ status }: { status: GarminStatus }) {
  const { chooseWhatToSend } = useGarminActions();
  return (
    <div className="form__group">
      <SettingsGroup title="Missed nights">
        <SettingsRow
          icon={<DownloadIcon />}
          tone="sleep"
          title="Fill missed nights from Garmin"
          checked={status.fill_nights}
          disabled={chooseWhatToSend.isPending}
          onCheckedChange={(next) => chooseWhatToSend.mutate({ fill_nights: next })}
        />
      </SettingsGroup>
      <p className="card__intro">
        When the Garmin Connect app syncs your watch first, Nimrag misses that night&apos;s sleep stages and HRV. Nimrag then fetches the night from your Garmin
        account each morning. Your watch&apos;s own files always win.
      </p>
      {status.fill_nights && status.last_filled_night ? <p className="card__intro">Last night filled: {fmtLongDay(status.last_filled_night)}</p> : null}
      {chooseWhatToSend.isError ? <Failure error={chooseWhatToSend.error} /> : null}
    </div>
  );
}

/** Everything mode: send the files Garmin Connect has not got yet, from a start date (oldest first). */
function SendMissing({ timezone }: { timezone: string }) {
  const { backfill } = useGarminActions();
  const [today] = useState(() => isoDay(Date.now(), timezone));
  const [since, setSince] = useState(() => isoDay(Date.now() - 7 * 86_400_000, timezone));
  const sinceOk = /^\d{4}-\d{2}-\d{2}$/.test(since) && since <= today;

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (sinceOk) backfill.mutate({ since, mode: 'all' });
      }}
      className="form__group"
      aria-label="Send what Garmin Connect is missing"
    >
      <TextField
        label="Files from the watch since"
        type="date"
        max={today}
        value={since}
        onChange={(e) => {
          setSince(e.target.value);
          backfill.reset();
        }}
        aria-invalid={since !== '' && !sinceOk ? true : undefined}
        hint={
          since !== '' && !sinceOk
            ? 'Pick a day up to today.'
            : 'New files are sent on their own. This queues the older ones too, oldest first, about one a second. Garmin skips what it already has.'
        }
      />
      <Button type="submit" disabled={!sinceOk || backfill.isPending} aria-busy={backfill.isPending}>
        {backfill.isPending ? 'Queuing' : 'Also send what Garmin Connect is missing'}
      </Button>
      {backfill.isSuccess ? (
        <p className="status-msg status-msg--ok" role="status">
          {backfill.data.queued === 0
            ? 'Nothing new to queue: those files are already queued or sent.'
            : `Queued ${fmtInt(backfill.data.queued)} ${backfill.data.queued === 1 ? 'file' : 'files'}.`}
        </p>
      ) : null}
      {backfill.isError ? <Failure error={backfill.error} /> : null}
    </form>
  );
}

/** Activities only: queue the activities already stored. */
function SendOlderActivities() {
  const { backfill } = useGarminActions();
  const [days, setDays] = useState('30');
  const n = Number(days);
  const daysOk = Number.isInteger(n) && n >= 1 && n <= 3650;

  return (
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
  );
}

function ConnectedTools({ status, timezone }: { status: GarminStatus; timezone: string }) {
  const { retry } = useGarminActions();

  return (
    <>
      <WhatToSend status={status} />
      <FillNights status={status} />
      <Counts s={status} timezone={timezone} />
      <div className="form__group" style={{ gap: 6 }}>
        <Button disabled={retry.isPending || status.failed === 0} onClick={() => retry.mutate()} aria-busy={retry.isPending}>
          Try the failed ones again
        </Button>
        {retry.isError ? <Failure error={retry.error} /> : null}
      </div>
      {status.send_all ? <SendMissing timezone={timezone} /> : <SendOlderActivities />}
    </>
  );
}

/** The warning and the login: shown only once the person chooses to turn sending on (or to log in again). */
function LoginPanel({ again, onLoggedIn }: { again: boolean; onLoggedIn: () => void }) {
  return (
    <Card padding="lg" aria-label="Log in to Garmin">
      <Notice tone="warn">
        The login is unofficial: it copies Garmin&apos;s phone app, may stop working without notice and may conflict with Garmin&apos;s terms. Your password is
        used once and never stored; only login tokens are kept, on your server.
      </Notice>
      <LoginForm again={again} onLoggedIn={onLoggedIn} />
      <ImportTokens />
    </Card>
  );
}

/**
 * Profile, Send to Garmin: the server sends the watch's files to the owner's Garmin Connect
 * account, activities only or everything. The page is a switch and a line of explanation; the login
 * form appears only when the switch is turned on, and turning it off again (after a login) asks first.
 * Once connected, the choice (and, for Everything, the offer to send older files) sits under the counts.
 */
export function GarminPage() {
  const query = useGarmin();
  const settings = useSettings();
  const timezone = settings.data?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone;
  const { logout } = useGarminActions();
  const [enabling, setEnabling] = useState(false);
  const [relogin, setRelogin] = useState(false);
  const [restart, setRestart] = useState(false);
  const [confirmOff, setConfirmOff] = useState(false);
  const s = query.data;
  const state: GarminState | null = s?.state ?? null;
  const on = state !== null && state !== 'off';

  return (
    <SubPage title="Send to Garmin" backTo="/profile">
      <p className="subpage__lede">Your server can send your watch&apos;s files to your own Garmin Connect account too: activities only, or everything from the watch.</p>
      {query.isPending ? (
        <Card padding="lg" aria-label="Loading the Garmin status">
          <Skeleton height={20} lines={3} />
        </Card>
      ) : null}
      {query.isError && !s ? (
        <Card padding="lg">
          <ErrorState title="Could not load the Garmin status" action={<Button onClick={() => void query.refetch()}>Try again</Button>}>
            {errorMessage(query.error)}
          </ErrorState>
        </Card>
      ) : null}
      {s && state ? (
        <>
          <SettingsGroup title="Garmin Connect">
            <SettingsRow
              icon={<SendIcon />}
              tone="steps"
              title="Send to Garmin"
              value={enabling && state === 'off' ? 'Log in to turn it on' : GARMIN_STATE_VALUE[state]}
              checked={on || enabling}
              onCheckedChange={(next) => {
                if (next) {
                  if (state === 'off') setEnabling(true);
                } else if (state === 'off') {
                  setEnabling(false);
                } else {
                  setConfirmOff(true);
                }
              }}
            />
          </SettingsGroup>

          {state === 'off' && enabling ? <LoginPanel again={false} onLoggedIn={() => setEnabling(false)} /> : null}

          {state === 'needs_mfa' && !restart ? (
            <Card padding="lg" aria-label="Garmin verification">
              <MfaForm onRestart={() => setRestart(true)} />
            </Card>
          ) : null}
          {state === 'needs_mfa' && restart ? <LoginPanel again onLoggedIn={() => setRestart(false)} /> : null}

          {state === 'needs_login' || state === 'paused' ? (
            <>
              <StatusBanner
                action={
                  relogin ? null : (
                    <Button onClick={() => setRelogin(true)}>Log in again</Button>
                  )
                }
              >
                {state === 'paused' ? 'Sending is paused: Garmin kept refusing the stored login.' : 'Garmin asks you to log in again.'}
              </StatusBanner>
              {relogin ? <LoginPanel again onLoggedIn={() => setRelogin(false)} /> : null}
            </>
          ) : null}

          {state === 'connected' ? (
            <Card padding="lg" aria-label="Sending status">
              <ConnectedTools status={s} timezone={timezone} />
            </Card>
          ) : null}

          {s.last_error ? (
            <p className="status-msg status-msg--error" role="status">
              Last error: {s.last_error}
            </p>
          ) : null}
        </>
      ) : null}
      {confirmOff ? (
        <ConfirmDialog
          title="Stop sending to Garmin?"
          consequence="New files stop going to Garmin Connect, and the stored login tokens are deleted from your server."
          confirmLabel="Turn off"
          pending={logout.isPending}
          error={logout.isError ? errorMessage(logout.error) : undefined}
          onCancel={() => setConfirmOff(false)}
          onConfirm={() =>
            logout.mutate(undefined, {
              onSuccess: () => {
                setConfirmOff(false);
                setEnabling(false);
                setRelogin(false);
                setRestart(false);
              },
            })
          }
        />
      ) : null}
    </SubPage>
  );
}
