import { useId, type Ref } from 'react';
import { TextField } from '../components';
import { cleanCode } from './passwords';
import { QrCode } from './QrCode';

interface CodeFieldProps {
  value: string;
  onChange: (code: string) => void;
  label?: string;
  hint?: string;
  id?: string;
  ref?: Ref<HTMLInputElement>;
  invalid?: boolean;
}

/** The 6-digit code from the authenticator app; phones offer the code from the SMS-style autofill. */
export function CodeField({ value, onChange, label = '6-digit code', hint, id, ref, invalid }: CodeFieldProps) {
  return (
    <TextField
      ref={ref}
      id={id}
      label={label}
      name="code"
      autoComplete="one-time-code"
      inputMode="numeric"
      pattern="[0-9]{6}"
      maxLength={6}
      autoCapitalize="off"
      spellCheck={false}
      value={value}
      onChange={(e) => onChange(cleanCode(e.target.value))}
      aria-invalid={invalid ? true : undefined}
      hint={hint}
    />
  );
}

/**
 * What a person needs to add the account to an authenticator app: the QR code, and the same
 * secret as text for apps that cannot scan.
 */
export function AuthenticatorPanel({ uri, secret }: { uri: string; secret: string }) {
  const secretId = useId();
  return (
    <div className="authenticator">
      <QrCode value={uri} label="QR code to scan with your authenticator app. The same secret is written below as text." />
      <div className="authenticator__text">
        <p className="stat__sub">Scan the code with an authenticator app (for example Aegis, Google Authenticator or 1Password). Cannot scan? Enter this key by hand, with the type set to time-based:</p>
        <label className="field__label" htmlFor={secretId}>
          Secret key
        </label>
        <input id={secretId} className="field__input authenticator__secret" readOnly value={secret} onFocus={(e) => e.currentTarget.select()} />
      </div>
    </div>
  );
}
