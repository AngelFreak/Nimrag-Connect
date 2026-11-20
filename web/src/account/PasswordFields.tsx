import { useState } from 'react';
import { TextField } from '../components';
import { MIN_PASSWORD_LEN } from './passwords';

interface Props {
  password: string;
  confirm: string;
  onPassword: (value: string) => void;
  onConfirm: (value: string) => void;
  label?: string;
  confirmLabel?: string;
  hint?: string;
}

/** A new password and its confirmation. The mismatch is only pointed out once the second field has text. */
export function NewPasswordFields({ password, confirm, onPassword, onConfirm, label = 'New password', confirmLabel = 'New password again', hint }: Props) {
  const [touched, setTouched] = useState(false);
  const mismatch = touched && confirm.length > 0 && password !== confirm;
  return (
    <>
      <TextField
        label={label}
        type="password"
        name="new-password"
        autoComplete="new-password"
        required
        value={password}
        onChange={(e) => onPassword(e.target.value)}
        hint={hint ?? `At least ${MIN_PASSWORD_LEN} characters.`}
      />
      <TextField
        label={confirmLabel}
        type="password"
        name="confirm-password"
        autoComplete="new-password"
        required
        value={confirm}
        onChange={(e) => {
          setTouched(true);
          onConfirm(e.target.value);
        }}
        aria-invalid={mismatch ? true : undefined}
        hint={mismatch ? 'The two passwords are not the same.' : undefined}
      />
    </>
  );
}
