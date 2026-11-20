import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, Ref, SelectHTMLAttributes } from 'react';
import { Link, type LinkProps } from 'react-router-dom';
import { useId } from 'react';

interface Common {
  variant?: 'primary' | 'secondary' | 'dashed';
  size?: 'md' | 'lg';
  block?: boolean;
}

function cls({ variant = 'secondary', size = 'md', block }: Common): string {
  return ['btn', variant === 'primary' && 'btn--primary', variant === 'dashed' && 'btn--dashed', size === 'lg' && 'btn--lg', block && 'btn--block']
    .filter(Boolean)
    .join(' ');
}

export function Button({ variant, size, block, className, type = 'button', ...rest }: Common & ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button type={type} className={[cls({ variant, size, block }), className].filter(Boolean).join(' ')} {...rest} />;
}

export function LinkButton({ variant, size, block, className, ...rest }: Common & LinkProps) {
  return <Link className={[cls({ variant, size, block }), className].filter(Boolean).join(' ')} {...rest} />;
}

interface FieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'id'> {
  label: ReactNode;
  hint?: ReactNode;
  id?: string;
  /** Ref to the `<input>` (focus it after a failed submit, read a file input). */
  ref?: Ref<HTMLInputElement>;
}

/** Labelled text input (48 px high, 16 px text so phones do not zoom on focus). */
export function TextField({ label, hint, id, ref, ...rest }: FieldProps) {
  const auto = useId();
  const inputId = id ?? auto;
  return (
    <div className="field">
      <label className="field__label" htmlFor={inputId}>
        {label}
      </label>
      <input ref={ref} id={inputId} className="field__input" aria-describedby={hint ? `${inputId}-hint` : undefined} {...rest} />
      {hint ? (
        <span className="field__hint" id={`${inputId}-hint`}>
          {hint}
        </span>
      ) : null}
    </div>
  );
}

interface SelectFieldProps extends Omit<SelectHTMLAttributes<HTMLSelectElement>, 'id'> {
  label: ReactNode;
  hint?: ReactNode;
  id?: string;
  ref?: Ref<HTMLSelectElement>;
}

/** Labelled native select with the same 48 px box as `TextField`. */
export function SelectField({ label, hint, id, ref, children, ...rest }: SelectFieldProps) {
  const auto = useId();
  const selectId = id ?? auto;
  return (
    <div className="field">
      <label className="field__label" htmlFor={selectId}>
        {label}
      </label>
      <select ref={ref} id={selectId} className="field__input" aria-describedby={hint ? `${selectId}-hint` : undefined} {...rest}>
        {children}
      </select>
      {hint ? (
        <span className="field__hint" id={`${selectId}-hint`}>
          {hint}
        </span>
      ) : null}
    </div>
  );
}

/** Row with a label on the left and a checkbox on the right (Devices: what the phone sends). */
export function CheckRow({ label, ...rest }: { label: ReactNode } & Omit<InputHTMLAttributes<HTMLInputElement>, 'type'>) {
  return (
    <label className="check-row">
      {label}
      <input type="checkbox" {...rest} />
    </label>
  );
}
