import { useEffect, useRef, useState, type FormEvent } from 'react';
import type { SetupStartResponse } from '../../api/generated';
import { classifyStepError, describeAccountError, describeStepError } from '../../account/errors';
import { AuthenticatorPanel, CodeField } from '../../account/TotpFields';
import { Button, Card, CardHeader, TextField } from '../../components';
import { useConfirmTwoFactor, useStartTwoFactor } from './queries';

type Phase = { kind: 'idle' } | { kind: 'password' } | { kind: 'code'; data: SetupStartResponse } | { kind: 'done' };

/** Step one: the password again (a stolen session must not be enough to swap the authenticator). */
function PasswordStep({ onStarted, onCancel }: { onStarted: (data: SetupStartResponse) => void; onCancel: () => void }) {
  const start = useStartTwoFactor();
  const [password, setPassword] = useState('');
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    input.current?.focus();
  }, []);

  function submit(e: FormEvent) {
    e.preventDefault();
    if (!password || start.isPending) return;
    start.mutate({ password }, { onSuccess: onStarted, onSettled: () => setPassword('') });
  }

  return (
    <form onSubmit={submit} noValidate style={{ display: 'flex', flexDirection: 'column', gap: 12 }} aria-label="Confirm your password">
      <p className="stat__sub">To set up two-factor again, confirm your password. The authenticator you use now keeps working until you finish.</p>
      <TextField
        ref={input}
        label="Your password"
        type="password"
        name="current-password"
        autoComplete="current-password"
        required
        value={password}
        onChange={(e) => setPassword(e.target.value)}
      />
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        <Button type="submit" variant="primary" disabled={!password || start.isPending} aria-busy={start.isPending}>
          {start.isPending ? 'Checking' : 'Continue'}
        </Button>
        <Button onClick={onCancel}>Cancel</Button>
      </div>
      {start.isError ? (
        <p className="status-msg status-msg--error" role="alert">
          {describeAccountError(start.error)}
        </p>
      ) : null}
    </form>
  );
}

/** Step two: the QR code and a code from the new authenticator. */
function CodeStep({ data, onDone, onRestart }: { data: SetupStartResponse; onDone: () => void; onRestart: (message: string) => void }) {
  const confirm = useConfirmTwoFactor();
  const [code, setCode] = useState('');
  const codeInput = useRef<HTMLInputElement>(null);

  function submit(e: FormEvent) {
    e.preventDefault();
    if (code.length !== 6 || confirm.isPending) return;
    confirm.mutate(
      { pending_token: data.pending_token, code },
      {
        onSuccess: onDone,
        onError: (err) => {
          if (classifyStepError(err) === 'expired') {
            onRestart('That step expired. Start again.');
            return;
          }
          setCode('');
          codeInput.current?.focus();
        },
      },
    );
  }

  return (
    <form onSubmit={submit} noValidate style={{ display: 'flex', flexDirection: 'column', gap: 14 }} aria-label="Set up the new authenticator">
      <AuthenticatorPanel uri={data.otpauth_uri} secret={data.secret} />
      <CodeField ref={codeInput} value={code} onChange={setCode} hint="The code the new entry in your authenticator app shows." />
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        <Button type="submit" variant="primary" disabled={code.length !== 6 || confirm.isPending} aria-busy={confirm.isPending}>
          {confirm.isPending ? 'Checking' : 'Confirm'}
        </Button>
        <Button onClick={() => onRestart('')}>Cancel</Button>
      </div>
      {confirm.isError && classifyStepError(confirm.error) !== 'expired' ? (
        <p className="status-msg status-msg--error" role="alert">
          {describeStepError(confirm.error)}
        </p>
      ) : null}
    </form>
  );
}

/** Settings, "Two-factor": swap the authenticator (password, QR code, confirm with a code). */
export function TwoFactorCard() {
  const [phase, setPhase] = useState<Phase>({ kind: 'idle' });
  const [notice, setNotice] = useState('');

  return (
    <Card padding="lg" aria-labelledby="two-factor-title">
      <CardHeader title="Two-factor" id="two-factor-title" aside="Codes from an authenticator app" />
      {phase.kind === 'idle' || phase.kind === 'done' ? (
        <>
          <p className="stat__sub">Logging in asks for a 6-digit code from your authenticator app. Got a new phone? Set up two-factor again here.</p>
          {phase.kind === 'done' ? (
            <p className="status-msg status-msg--ok" role="status">
              Two-factor is set up with the new authenticator. The old one no longer works.
            </p>
          ) : null}
          {notice ? (
            <p className="status-msg status-msg--error" role="alert">
              {notice}
            </p>
          ) : null}
          <Button
            onClick={() => {
              setNotice('');
              setPhase({ kind: 'password' });
            }}
          >
            Set up two-factor again
          </Button>
        </>
      ) : null}
      {phase.kind === 'password' ? <PasswordStep onStarted={(data) => setPhase({ kind: 'code', data })} onCancel={() => setPhase({ kind: 'idle' })} /> : null}
      {phase.kind === 'code' ? (
        <CodeStep
          data={phase.data}
          onDone={() => setPhase({ kind: 'done' })}
          onRestart={(message) => {
            setNotice(message);
            setPhase({ kind: 'idle' });
          }}
        />
      ) : null}
    </Card>
  );
}
