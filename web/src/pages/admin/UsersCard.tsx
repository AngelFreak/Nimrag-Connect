import { useState } from 'react';
import type { UserInfo } from '../../api/generated';
import { useMe } from '../../account/queries';
import { Button, Card, CardHeader, Chip, EmptyState, ErrorState, Skeleton } from '../../components';
import { errorMessage } from '../system/errors';
import { useSetDisabled, useUsers } from './queries';
import { CreateUserDialog, DeleteUserDialog, DisableUserDialog, ResetPasswordDialog, ResetTwoFactorDialog, describeAdminError } from './UserDialogs';

type Open = { kind: 'create' } | { kind: 'reset-password' | 'reset-2fa' | 'disable' | 'delete'; user: UserInfo } | null;

function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString('en-GB')} ${n === 1 ? one : many}`;
}

/** One line of facts: two-factor state, pending password change, tokens, files. */
export function userFacts(user: UserInfo): string {
  return [
    user.totp_enabled ? 'Two-factor on' : 'Two-factor not set up',
    user.must_change_password ? 'temporary password' : null,
    plural(user.active_tokens, 'active token', 'active tokens'),
    plural(user.files, 'file', 'files'),
  ]
    .filter(Boolean)
    .join(' · ');
}

function UserRow({ user, self, onOpen }: { user: UserInfo; self: boolean; onOpen: (open: Open) => void }) {
  const enable = useSetDisabled();
  return (
    <li className="user-row" data-disabled={user.disabled ? 'true' : undefined}>
      <div className="user-row__main">
        <p className="user-row__name">
          <span>{user.username}</span>
          <Chip>{user.role === 'admin' ? 'Admin' : 'User'}</Chip>
          {self ? <Chip>You</Chip> : null}
          {user.disabled ? <Chip>Disabled</Chip> : null}
        </p>
        <p className="user-row__facts">{userFacts(user)}</p>
        {enable.isError ? (
          <p className="status-msg status-msg--error" role="alert">
            {describeAdminError(enable.error)}
          </p>
        ) : null}
      </div>
      {/* Your own password and two-factor are changed in Settings; the server refuses them here. */}
      {self ? null : (
        <div className="user-row__actions">
          <Button aria-label={`Reset password of ${user.username}`} onClick={() => onOpen({ kind: 'reset-password', user })}>
            Reset password
          </Button>
          <Button aria-label={`Reset two-factor of ${user.username}`} onClick={() => onOpen({ kind: 'reset-2fa', user })}>
            Reset 2FA
          </Button>
          {user.disabled ? (
            <Button aria-label={`Enable ${user.username}`} disabled={enable.isPending} onClick={() => enable.mutate({ id: user.id, disabled: false })}>
              Enable
            </Button>
          ) : (
            <Button aria-label={`Disable ${user.username}`} onClick={() => onOpen({ kind: 'disable', user })}>
              Disable
            </Button>
          )}
          <Button aria-label={`Delete ${user.username}`} onClick={() => onOpen({ kind: 'delete', user })}>
            Delete
          </Button>
        </div>
      )}
    </li>
  );
}

/** Admin: every account, with the actions that manage it. */
export function UsersCard() {
  const query = useUsers();
  const me = useMe();
  const [open, setOpen] = useState<Open>(null);
  const close = () => setOpen(null);
  const users = query.data?.users ?? [];

  return (
    <Card padding="lg" aria-labelledby="users-title">
      <CardHeader title="Users" id="users-title" aside={query.data ? plural(users.length, 'account', 'accounts') : undefined} />
      {query.isPending ? <Skeleton height={20} lines={3} /> : null}
      {query.isError && !query.data ? (
        <ErrorState title="Could not load the users" action={<Button onClick={() => void query.refetch()}>Try again</Button>}>
          {errorMessage(query.error)}
        </ErrorState>
      ) : null}
      {query.data ? (
        users.length === 0 ? (
          <EmptyState title="No users yet">Create the first user. They get the data and the app.</EmptyState>
        ) : (
          <ul className="user-list" aria-label="Users">
            {users.map((u) => (
              <UserRow key={u.id} user={u} self={me.data?.username === u.username} onOpen={setOpen} />
            ))}
          </ul>
        )
      ) : null}
      <Button variant="dashed" size="lg" onClick={() => setOpen({ kind: 'create' })}>
        Create user
      </Button>
      {open?.kind === 'create' ? <CreateUserDialog onClose={close} /> : null}
      {open?.kind === 'reset-password' ? <ResetPasswordDialog user={open.user} onClose={close} /> : null}
      {open?.kind === 'reset-2fa' ? <ResetTwoFactorDialog user={open.user} onClose={close} /> : null}
      {open?.kind === 'disable' ? <DisableUserDialog user={open.user} onClose={close} /> : null}
      {open?.kind === 'delete' ? <DeleteUserDialog user={open.user} onClose={close} /> : null}
    </Card>
  );
}
