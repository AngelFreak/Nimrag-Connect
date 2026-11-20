import type { InputHTMLAttributes, ReactNode } from 'react';

/**
 * On/off switch with a label (HRV: show overnight averages). A real checkbox with role "switch",
 * drawn as a pill track with a round thumb in the app's colours; Space toggles it, focus shows a ring.
 */
export function Toggle({ label, className, ...rest }: { label: ReactNode } & Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'role'>) {
  return (
    <label className={['toggle', className].filter(Boolean).join(' ')}>
      <input type="checkbox" role="switch" className="toggle__input" {...rest} />
      <span className="toggle__track" aria-hidden="true" />
      <span className="toggle__label">{label}</span>
    </label>
  );
}
