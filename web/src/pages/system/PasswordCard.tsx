import { useState, type FormEvent } from 'react';
import { describeAccountError } from '../../account/errors';
import { NewPasswordFields } from '../../account/PasswordFields';
import { passwordProblem } from '../../account/passwords';
import { Button, Card, CardHeader, TextField } from '../../components';
import { useChangePassword } from './queries';

/** Settings, "Change password": POST /api/me/password. The other devices are logged out. */
export function PasswordCard() {
  const change = useChangePassword();
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [confirm, setConfirm] = useState('');
  const [saved, setSaved] = useState(false);
  const problem = passwordProblem(next, confirm);

  function submit(e: FormEvent) {
    e.preventDefault();
    if (problem || !current || change.isPending) return;
    setSaved(false);
    change.mutate(
      { current_password: current, new_password: next },
      {
        onSuccess: () => {
          setSaved(true);
          setNext('');
          setConfirm('');
        },
        onSettled: () => setCurrent(''),
      },
    );
  }

  return (
    <Card padding="lg" aria-labelledby="password-title">
      <CardHeader title="Change password" id="password-title" level={3} />
      <p className="card__intro">Your other devices are logged out when you change it.</p>
      <form onSubmit={submit} noValidate className="form" aria-labelledby="password-title">
        <TextField
          label="Current password"
          type="password"
          name="current-password"
          autoComplete="current-password"
          required
          value={current}
          onChange={(e) => {
            setCurrent(e.target.value);
            setSaved(false);
          }}
        />
        <NewPasswordFields password={next} confirm={confirm} onPassword={setNext} onConfirm={setConfirm} />
        <Button type="submit" size="lg" variant="primary" disabled={!current || problem !== null || change.isPending} aria-busy={change.isPending}>
          {change.isPending ? 'Saving' : 'Change password'}
        </Button>
        {saved ? (
          <p className="status-msg status-msg--ok" role="status">
            Password changed. You stay logged in here; every other device was logged out.
          </p>
        ) : null}
        {change.isError ? (
          <p className="status-msg status-msg--error" role="alert">
            {describeAccountError(change.error)}
          </p>
        ) : null}
      </form>
    </Card>
  );
}
