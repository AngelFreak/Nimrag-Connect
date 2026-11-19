import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ApiError } from '../../api/client';
import type { NewTokenResponse } from '../../api/generated';
import type { TokenInfo } from '../../api/generated/TokenInfo';
import { Button, Card, CardHeader, ErrorState, Skeleton, Table, TextField } from '../../components';
import { fmtDateTime } from '../today/format';
import { errorMessage } from './errors';
import { useCreateToken, useRevokeToken, useTokens } from './queries';

/** Plain words for a refused token request. A wrong owner password is 403. */
export function describeTokenError(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 403) return 'Wrong password.';
    if (error.status === 429) return 'Too many attempts. Wait a little and try again.';
  }
  return errorMessage(error);
}

/** The token is shown once, with a warning and a copy button. Closing it drops the secret from memory. */
function NewTokenNotice({ token, onClose }: { token: NewTokenResponse; onClose: () => void }) {
  const [copied, setCopied] = useState<'yes' | 'no' | null>(null);
  const box = useRef<HTMLTextAreaElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    heading.current?.focus();
  }, []);

  async function copy() {
    try {
      await navigator.clipboard.writeText(token.token);
      setCopied('yes');
    } catch {
      // No clipboard permission (plain http): select the text so the owner can copy it by hand.
      box.current?.focus();
      box.current?.select();
      setCopied('no');
    }
  }

  return (
    <div className="token-secret" role="group" aria-labelledby="token-secret-title">
      <h3 className="token-secret__title" id="token-secret-title" ref={heading} tabIndex={-1}>
        Token created: {token.name}
      </h3>
      <p className="status-msg status-msg--error" role="alert">
        Copy this token now. It is shown only once and cannot be looked up again. Anyone who has it can upload files to your server.
      </p>
      <label className="field__label" htmlFor="token-secret-value">
        Token
      </label>
      <textarea id="token-secret-value" ref={box} className="field__input token-secret__value" readOnly rows={2} value={token.token} onFocus={(e) => e.currentTarget.select()} />
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', alignItems: 'center' }}>
        <Button variant="primary" onClick={() => void copy()}>
          Copy token
        </Button>
        <Button onClick={onClose}>I have saved it</Button>
      </div>
      <p className="stat__sub" role="status">
        {copied === 'yes' ? 'Copied to the clipboard.' : copied === 'no' ? 'Could not copy automatically. The token is selected: copy it by hand.' : ''}
      </p>
    </div>
  );
}

function CreateForm({ onCreated, onCancel }: { onCreated: (t: NewTokenResponse) => void; onCancel: () => void }) {
  const create = useCreateToken();
  const [name, setName] = useState('');
  const [password, setPassword] = useState('');
  const nameInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    nameInput.current?.focus();
  }, []);

  function submit(e: FormEvent) {
    e.preventDefault();
    create.mutate(
      { name: name.trim(), password },
      {
        onSuccess: (t) => {
          setName('');
          onCreated(t);
        },
        onSettled: () => setPassword(''),
      },
    );
  }

  return (
    <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: 12 }} aria-label="Create a device token">
      <TextField ref={nameInput} label="Name" maxLength={64} value={name} onChange={(e) => setName(e.target.value)} hint='What it is for, for example "Pixel 9a".' required />
      <TextField
        label="Your password"
        type="password"
        autoComplete="current-password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        hint="Asked again so a stolen session cannot mint upload tokens."
        required
      />
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        <Button type="submit" variant="primary" disabled={create.isPending || !name.trim() || !password} aria-busy={create.isPending}>
          {create.isPending ? 'Creating' : 'Create token'}
        </Button>
        <Button onClick={onCancel}>Cancel</Button>
      </div>
      {create.isError ? (
        <p className="status-msg status-msg--error" role="alert">
          {describeTokenError(create.error)}
        </p>
      ) : null}
    </form>
  );
}

function RevokeButton({ token }: { token: TokenInfo }) {
  const revoke = useRevokeToken();
  const [confirm, setConfirm] = useState(false);
  if (!token.active) return null;
  if (!confirm) {
    return (
      <Button aria-label={`Revoke token ${token.name}`} onClick={() => setConfirm(true)}>
        Revoke
      </Button>
    );
  }
  return (
    <span style={{ display: 'inline-flex', gap: 8, alignItems: 'center' }}>
      <Button aria-label={`Confirm revoking token ${token.name}`} onClick={() => revoke.mutate(token.id, { onSuccess: () => setConfirm(false) })} disabled={revoke.isPending}>
        {revoke.isError ? 'Try again' : 'Yes, revoke'}
      </Button>
      <Button onClick={() => setConfirm(false)}>Cancel</Button>
    </span>
  );
}

/** Settings, "Device tokens": the credentials phones and tools use to upload files. */
export function TokensCard({ timezone }: { timezone: string }) {
  const query = useTokens();
  const [creating, setCreating] = useState(false);
  const [fresh, setFresh] = useState<NewTokenResponse | null>(null);
  const tokens = query.data?.tokens ?? [];

  return (
    <Card padding="lg" aria-labelledby="tokens-title">
      <CardHeader title="Device tokens" id="tokens-title" aside="Phones and tools that upload files" />
      {query.isPending ? <Skeleton height={20} lines={3} /> : null}
      {query.isError && !query.data ? (
        <ErrorState title="Could not load the device tokens" action={<Button onClick={() => void query.refetch()}>Try again</Button>}>
          {errorMessage(query.error)}
        </ErrorState>
      ) : null}
      {query.data ? (
        tokens.length === 0 ? (
          <p className="stat__sub">No device tokens yet. Create one for each phone or tool that uploads files.</p>
        ) : (
          <Table
            caption="Device tokens"
            wide
            rows={tokens}
            rowKey={(t) => String(t.id)}
            columns={[
              { key: 'name', header: 'Name', cell: (t) => t.name },
              { key: 'created', header: 'Created', cell: (t) => fmtDateTime(t.created_at, timezone) },
              { key: 'used', header: 'Last used', cell: (t) => fmtDateTime(t.last_used_at, timezone) ?? 'Never' },
              { key: 'status', header: 'Status', cell: (t) => (t.active ? 'Active' : `Revoked ${fmtDateTime(t.revoked_at, timezone) ?? ''}`.trim()) },
              { key: 'act', header: <span className="sr-only">Actions</span>, align: 'end', cell: (t) => <RevokeButton token={t} /> },
            ]}
          />
        )
      ) : null}
      {fresh ? <NewTokenNotice token={fresh} onClose={() => setFresh(null)} /> : null}
      {creating && !fresh ? (
        <CreateForm
          onCreated={(t) => {
            setFresh(t);
            setCreating(false);
          }}
          onCancel={() => setCreating(false)}
        />
      ) : null}
      {!creating && !fresh ? (
        <Button variant="dashed" size="lg" onClick={() => setCreating(true)}>
          Create token
        </Button>
      ) : null}
    </Card>
  );
}
