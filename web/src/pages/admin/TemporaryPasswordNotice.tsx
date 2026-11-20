import { useRef, useState } from 'react';
import { Button } from '../../components';

/**
 * A temporary password is shown once, with a warning and a copy button. It is only ever held by
 * the component that shows it: closing the dialog drops it.
 */
export function TemporaryPasswordNotice({ username, password, heading }: { username: string; password: string; heading: string }) {
  const [copied, setCopied] = useState<'yes' | 'no' | null>(null);
  const box = useRef<HTMLInputElement>(null);

  async function copy() {
    try {
      await navigator.clipboard.writeText(password);
      setCopied('yes');
    } catch {
      // No clipboard permission (plain http): select the text so it can be copied by hand.
      box.current?.focus();
      box.current?.select();
      setCopied('no');
    }
  }

  return (
    <div className="token-secret" role="group" aria-label={heading}>
      <p className="status-msg status-msg--error" role="alert">
        Copy this temporary password now. It is shown only once and cannot be looked up again. Give it to {username}; they must replace it the first time they log in.
      </p>
      <label className="field__label" htmlFor="temporary-password-value">
        Temporary password for {username}
      </label>
      <input id="temporary-password-value" ref={box} className="field__input token-secret__value temp-password" readOnly value={password} onFocus={(e) => e.currentTarget.select()} />
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', alignItems: 'center' }}>
        <Button variant="primary" onClick={() => void copy()}>
          Copy password
        </Button>
      </div>
      <p className="stat__sub" role="status">
        {copied === 'yes' ? 'Copied to the clipboard.' : copied === 'no' ? 'Could not copy automatically. The password is selected: copy it by hand.' : ''}
      </p>
    </div>
  );
}
