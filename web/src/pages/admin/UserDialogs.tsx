import { useId, useState, type FormEvent } from 'react';
import { ApiError } from '../../api/client';
import type { NewUserResponse, TemporaryPassword, UserInfo } from '../../api/generated';
import type { Role } from '../../api/generated/Role';
import { USERNAME_MAX, USERNAME_MIN, normaliseUsername, usernameProblem } from '../../account/passwords';
import { Button, Modal, SelectField, TextField } from '../../components';
import { errorMessage } from '../system/errors';
import { useCreateUser, useDeleteUser, useResetPassword, useResetTwoFactor, useSetDisabled } from './queries';
import { TemporaryPasswordNotice } from './TemporaryPasswordNotice';

/** The server's own sentence for a refused change ("this is the last active admin"), plain words otherwise. */
export function describeAdminError(error: unknown): string {
  if (error instanceof ApiError && error.status === 429) return 'Too many attempts. Wait a little and try again.';
  return errorMessage(error);
}

function ErrorLine({ error }: { error: unknown }) {
  if (!error) return null;
  return (
    <p className="status-msg status-msg--error" role="alert">
      {describeAdminError(error)}
    </p>
  );
}

interface DialogProps {
  onClose: () => void;
}

/** New user: username and role, then the temporary password, once. */
export function CreateUserDialog({ onClose }: DialogProps) {
  const create = useCreateUser();
  const formId = useId();
  const [username, setUsername] = useState('');
  const [role, setRole] = useState<Role>('user');
  const [created, setCreated] = useState<NewUserResponse | null>(null);
  const problem = username ? usernameProblem(username) : 'Choose a username.';

  function submit(e: FormEvent) {
    e.preventDefault();
    if (problem || create.isPending) return;
    create.mutate({ username: normaliseUsername(username), role }, { onSuccess: setCreated });
  }

  if (created) {
    return (
      <Modal
        title={`User created: ${created.user.username}`}
        description="Give them the temporary password. At the first login they replace it and set up two-factor."
        onClose={onClose}
        footer={
          <Button variant="primary" onClick={onClose}>
            I have copied it
          </Button>
        }
      >
        <TemporaryPasswordNotice username={created.user.username} password={created.temporary_password} heading="Temporary password" />
      </Modal>
    );
  }

  return (
    <Modal
      title="Create a user"
      description="The server makes a temporary password. The new user logs in with it, chooses their own password and sets up two-factor."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" form={formId} variant="primary" disabled={problem !== null || create.isPending} aria-busy={create.isPending}>
            {create.isPending ? 'Creating' : 'Create user'}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} noValidate style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        <TextField
          label="Username"
          name="new-username"
          autoComplete="off"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
          required
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          hint={username && problem ? problem : `${USERNAME_MIN} to ${USERNAME_MAX} characters: a to z, digits, '.', '_' and '-'. Capitals are made lower case.`}
          aria-invalid={username && problem ? true : undefined}
        />
        <SelectField label="Role" value={role} onChange={(e) => setRole(e.target.value as Role)} hint={role === 'admin' ? 'An admin manages accounts and has no data of their own.' : 'A user has the data and the app.'}>
          <option value="user">User</option>
          <option value="admin">Admin</option>
        </SelectField>
        <ErrorLine error={create.error} />
      </form>
    </Modal>
  );
}

/** A new temporary password for someone who lost theirs; shown once. */
export function ResetPasswordDialog({ user, onClose }: DialogProps & { user: UserInfo }) {
  const reset = useResetPassword();
  const [result, setResult] = useState<TemporaryPassword | null>(null);

  if (result) {
    return (
      <Modal
        title={`New temporary password for ${user.username}`}
        description="Their old password and all their sessions no longer work."
        onClose={onClose}
        footer={
          <Button variant="primary" onClick={onClose}>
            I have copied it
          </Button>
        }
      >
        <TemporaryPasswordNotice username={user.username} password={result.temporary_password} heading="Temporary password" />
      </Modal>
    );
  }

  return (
    <Modal
      title={`Reset the password of ${user.username}?`}
      description="The server sets a new temporary password and signs them out everywhere. They must replace it at the next login."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={reset.isPending} aria-busy={reset.isPending} onClick={() => reset.mutate(user.id, { onSuccess: setResult })}>
            {reset.isPending ? 'Resetting' : 'Reset password'}
          </Button>
        </>
      }
    >
      <ErrorLine error={reset.error} />
    </Modal>
  );
}

/** The user enrols two-factor again at the next login. */
export function ResetTwoFactorDialog({ user, onClose }: DialogProps & { user: UserInfo }) {
  const reset = useResetTwoFactor();
  const [done, setDone] = useState(false);

  return (
    <Modal
      title={done ? 'Two-factor reset' : `Reset two-factor for ${user.username}?`}
      description={
        done
          ? `${user.username} sets up two-factor again at the next login, with the password they have now.`
          : 'Use this when they lost their phone. They are signed out everywhere and set up two-factor again at the next login.'
      }
      onClose={onClose}
      footer={
        done ? (
          <Button variant="primary" onClick={onClose}>
            Close
          </Button>
        ) : (
          <>
            <Button onClick={onClose}>Cancel</Button>
            <Button variant="primary" disabled={reset.isPending} aria-busy={reset.isPending} onClick={() => reset.mutate(user.id, { onSuccess: () => setDone(true) })}>
              {reset.isPending ? 'Resetting' : 'Reset two-factor'}
            </Button>
          </>
        )
      }
    >
      <ErrorLine error={reset.error} />
    </Modal>
  );
}

/** Disabling signs the user out and refuses their logins. Their data stays. */
export function DisableUserDialog({ user, onClose }: DialogProps & { user: UserInfo }) {
  const disable = useSetDisabled();
  return (
    <Modal
      title={`Disable ${user.username}?`}
      description="They are signed out everywhere and cannot log in until you enable the account again. Their data and device tokens stay as they are."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={disable.isPending} aria-busy={disable.isPending} onClick={() => disable.mutate({ id: user.id, disabled: true }, { onSuccess: onClose })}>
            {disable.isPending ? 'Disabling' : 'Disable user'}
          </Button>
        </>
      }
    >
      <ErrorLine error={disable.error} />
    </Modal>
  );
}

function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString('en-GB')} ${n === 1 ? one : many}`;
}

/**
 * Delete in one step for a user without data, in two for a user with data: first what will
 * happen to it, then the confirmation that sends `confirm=true`. Original files are never deleted.
 */
export function DeleteUserDialog({ user, onClose }: DialogProps & { user: UserInfo }) {
  const remove = useDeleteUser();
  const [step, setStep] = useState<'ask' | 'confirm'>('ask');

  if (!user.has_data) {
    return (
      <Modal
        title={`Delete ${user.username}?`}
        description="The account is removed for good. It has no data."
        onClose={onClose}
        footer={
          <>
            <Button onClick={onClose}>Cancel</Button>
            <Button variant="primary" disabled={remove.isPending} aria-busy={remove.isPending} onClick={() => remove.mutate({ id: user.id, confirm: false }, { onSuccess: onClose })}>
              {remove.isPending ? 'Deleting' : 'Delete user'}
            </Button>
          </>
        }
      >
        <ErrorLine error={remove.error} />
      </Modal>
    );
  }

  const what = `${user.username} has ${plural(user.files, 'file', 'files')} and ${plural(user.active_tokens, 'active device token', 'active device tokens')}.`;

  if (step === 'ask') {
    return (
      <Modal
        title={`Delete ${user.username}?`}
        description={`${what} Deleting the user does not delete any file.`}
        onClose={onClose}
        footer={
          <>
            <Button onClick={onClose}>Cancel</Button>
            <Button variant="primary" onClick={() => setStep('confirm')}>
              Continue
            </Button>
          </>
        }
      >
        <ul className="modal__list">
          <li>Their files and device tokens go back to Unassigned data. The original files are never deleted, and you can hand them to another user.</li>
          <li>Their own records are removed: preferences, body entries, edited sets and the Garmin Connect link.</li>
          <li>They are signed out everywhere and the account is gone.</li>
        </ul>
      </Modal>
    );
  }

  return (
    <Modal
      title={`Delete ${user.username} and unassign their data?`}
      description="This cannot be undone. Their files and device tokens become unassigned; their records are removed."
      onClose={onClose}
      footer={
        <>
          <Button onClick={() => setStep('ask')}>Back</Button>
          <Button variant="primary" disabled={remove.isPending} aria-busy={remove.isPending} onClick={() => remove.mutate({ id: user.id, confirm: true }, { onSuccess: onClose })}>
            {remove.isPending ? 'Deleting' : 'Delete and unassign'}
          </Button>
        </>
      }
    >
      <ErrorLine error={remove.error} />
    </Modal>
  );
}
