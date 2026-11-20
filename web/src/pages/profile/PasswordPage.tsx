import { useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { describeAccountError } from '../../account/errors';
import { NewPasswordFields } from '../../account/PasswordFields';
import { passwordProblem } from '../../account/passwords';
import { Button, Card, TextField } from '../../components';
import { useChangePassword } from '../system/queries';
import { SubPage } from './common';

/** What the profile page says after a password change (it arrives in the navigation state). */
export const PASSWORD_CHANGED = 'Password changed. Your other devices were logged out.';

/**
 * Profile, Password: POST /api/me/password. The other devices are logged out and this session
 * continues. On success the person goes back to the profile with a notice, so the form is never left
 * open with a password in it.
 */
export function PasswordPage() {
  const change = useChangePassword();
  const navigate = useNavigate();
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [confirm, setConfirm] = useState('');
  const problem = passwordProblem(next, confirm);

  function submit(e: FormEvent) {
    e.preventDefault();
    if (problem || !current || change.isPending) return;
    change.mutate(
      { current_password: current, new_password: next },
      {
        onSuccess: () => void navigate('/profile', { replace: true, state: { notice: PASSWORD_CHANGED } }),
        onSettled: () => setCurrent(''),
      },
    );
  }

  return (
    <SubPage title="Password" backTo="/profile">
      <p className="subpage__lede">Your other devices are logged out when you change it. This one stays logged in.</p>
      <Card padding="lg">
        <form onSubmit={submit} noValidate className="form" aria-label="Change password">
          <TextField
            label="Current password"
            type="password"
            name="current-password"
            autoComplete="current-password"
            required
            value={current}
            onChange={(e) => {
              setCurrent(e.target.value);
              change.reset();
            }}
          />
          <NewPasswordFields password={next} confirm={confirm} onPassword={setNext} onConfirm={setConfirm} />
          <Button type="submit" size="lg" variant="primary" disabled={!current || problem !== null || change.isPending} aria-busy={change.isPending}>
            {change.isPending ? 'Saving' : 'Change password'}
          </Button>
          {change.isError ? (
            <p className="status-msg status-msg--error" role="alert">
              {describeAccountError(change.error)}
            </p>
          ) : null}
        </form>
      </Card>
    </SubPage>
  );
}
