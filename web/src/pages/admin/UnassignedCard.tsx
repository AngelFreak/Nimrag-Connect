import { useState, type FormEvent } from 'react';
import type { AssignRequest, AssignResponse, UnassignedResponse, UserInfo } from '../../api/generated';
import type { LegacyData } from '../../api/generated/LegacyData';
import { Button, Card, CardHeader, CheckRow, EmptyState, ErrorState, SelectField, Skeleton } from '../../components';
import { fmtDateTime } from '../today/format';
import { errorMessage } from '../system/errors';
import { useAssign, useUnassigned, useUsers } from './queries';
import { describeAdminError } from './UserDialogs';

function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString('en-GB')} ${n === 1 ? one : many}`;
}

/** Whether anything from before accounts is left to hand over. */
export function hasLegacy(legacy: LegacyData): boolean {
  return legacy.settings || legacy.body_entries > 0 || legacy.strength_edits > 0 || legacy.garmin_account;
}

function legacyText(legacy: LegacyData): string {
  return [
    legacy.settings ? 'preferences' : null,
    legacy.body_entries > 0 ? plural(legacy.body_entries, 'body entry', 'body entries') : null,
    legacy.strength_edits > 0 ? plural(legacy.strength_edits, 'edited set', 'edited sets') : null,
    legacy.garmin_account ? 'the Garmin Connect link' : null,
  ]
    .filter(Boolean)
    .join(', ');
}

export function assignSummary(done: AssignResponse, username: string): string {
  const parts = [plural(done.tokens_moved, 'device token', 'device tokens'), plural(done.files_moved, 'file', 'files')];
  if (done.legacy_adopted) parts.push('the records from before accounts');
  return `Gave ${parts.join(', ')} to ${username}.`;
}

function AssignForm({ data, users, timezone, setDone }: { data: UnassignedResponse; users: UserInfo[]; timezone: string; setDone: (message: string | null) => void }) {
  const assign = useAssign();
  const targets = users.filter((u) => u.role === 'user' && !u.disabled);
  const [tokens, setTokens] = useState<number[]>([]);
  const [unattributed, setUnattributed] = useState(false);
  const [legacy, setLegacy] = useState(false);
  const [target, setTarget] = useState('');
  const legacyLeft = hasLegacy(data.legacy);
  // A token that has left the list (assigned elsewhere meanwhile) is no longer selected.
  const picked = tokens.filter((id) => data.tokens.some((t) => t.id === id));
  const something = picked.length > 0 || (unattributed && data.unattributed_files > 0) || (legacy && legacyLeft);

  function toggle(id: number) {
    setDone(null);
    setTokens((t) => (t.includes(id) ? t.filter((x) => x !== id) : [...t, id]));
  }

  function submit(e: FormEvent) {
    e.preventDefault();
    const user = targets.find((u) => String(u.id) === target);
    if (!user || !something || assign.isPending) return;
    const body: AssignRequest = { token_ids: picked, user_id: user.id };
    if (unattributed && data.unattributed_files > 0) body.include_unattributed = true;
    if (legacy && legacyLeft) body.adopt_legacy = true;
    assign.mutate(body, {
      onSuccess: (res) => {
        setDone(assignSummary(res, user.username));
        setTokens([]);
        setUnattributed(false);
        setLegacy(false);
      },
    });
  }

  return (
    <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: 14 }} aria-label="Assign unassigned data">
      <p className="stat__sub">
        Device tokens made before accounts, or on the command line, wait here with the files they uploaded. Give them to the user they belong to: their phone then uploads into that user&apos;s account.
      </p>
      {data.tokens.length > 0 ? (
        <fieldset className="assign-group">
          <legend className="field__label">Device tokens</legend>
          {data.tokens.map((t) => (
            <CheckRow
              key={t.id}
              label={
                <span>
                  <span className="assign-group__name">{t.name}</span>
                  <span className="assign-group__meta">
                    {plural(t.files, 'file', 'files')} · {fmtDateTime(t.last_used_at, timezone) ? `last used ${fmtDateTime(t.last_used_at, timezone)}` : 'never used'}
                  </span>
                </span>
              }
              checked={picked.includes(t.id)}
              onChange={() => toggle(t.id)}
            />
          ))}
        </fieldset>
      ) : null}
      {data.unattributed_files > 0 ? (
        <CheckRow
          label={
            <span>
              <span className="assign-group__name">{plural(data.unattributed_files, 'unattributed file', 'unattributed files')}</span>
              <span className="assign-group__meta">Imported on the command line, or uploaded by a revoked or unknown device. Tick to give them to the user too.</span>
            </span>
          }
          checked={unattributed}
          onChange={(e) => {
            setDone(null);
            setUnattributed(e.target.checked);
          }}
        />
      ) : null}
      {legacyLeft ? (
        <CheckRow
          label={
            <span>
              <span className="assign-group__name">Records from before accounts</span>
              <span className="assign-group__meta">{legacyText(data.legacy)}. Refused when the user already has records of their own.</span>
            </span>
          }
          checked={legacy}
          onChange={(e) => {
            setDone(null);
            setLegacy(e.target.checked);
          }}
        />
      ) : null}
      {targets.length === 0 ? (
        <p className="stat__sub">There is no user to give this to yet. Create a user above first.</p>
      ) : (
        <SelectField
          label="Give to"
          value={target}
          onChange={(e) => {
            setDone(null);
            setTarget(e.target.value);
          }}
        >
          <option value="">Choose a user</option>
          {targets.map((u) => (
            <option key={u.id} value={u.id}>
              {u.username}
            </option>
          ))}
        </SelectField>
      )}
      <Button type="submit" size="lg" variant="primary" disabled={!target || !something || assign.isPending} aria-busy={assign.isPending}>
        {assign.isPending ? 'Assigning' : 'Assign'}
      </Button>
      {assign.isError ? (
        <p className="status-msg status-msg--error" role="alert">
          {describeAdminError(assign.error)}
        </p>
      ) : null}
    </form>
  );
}

/** Admin: device tokens and files nobody owns yet, and the way to hand them to a user. */
export function UnassignedCard({ timezone }: { timezone: string }) {
  const query = useUnassigned();
  const users = useUsers();
  const data = query.data;
  const [done, setDone] = useState<string | null>(null);
  const empty = data ? data.tokens.length === 0 && data.unattributed_files === 0 && !hasLegacy(data.legacy) : false;

  return (
    <Card padding="lg" aria-labelledby="unassigned-title">
      <CardHeader title="Unassigned data" id="unassigned-title" aside="Tokens and files nobody owns yet" />
      {query.isPending ? <Skeleton height={20} lines={3} /> : null}
      {query.isError && !data ? (
        <ErrorState title="Could not load the unassigned data" action={<Button onClick={() => void query.refetch()}>Try again</Button>}>
          {errorMessage(query.error)}
        </ErrorState>
      ) : null}
      {done ? (
        <p className="status-msg status-msg--ok" role="status">
          {done}
        </p>
      ) : null}
      {data && empty ? <EmptyState title="Nothing is unassigned">Every device token and file belongs to a user. A token made on the command line without a user shows up here.</EmptyState> : null}
      {data && !empty ? <AssignForm data={data} users={users.data?.users ?? []} timezone={timezone} setDone={setDone} /> : null}
    </Card>
  );
}
