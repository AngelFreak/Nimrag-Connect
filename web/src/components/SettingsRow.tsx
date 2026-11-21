import { Children, useId, type ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { NextIcon } from './icons';

export type RowTone = 'accent' | 'energy' | 'sleep' | 'warn' | 'heart' | 'steps' | 'muted';

interface RowBase {
  /** One of the stroke icons from `icons.tsx`; it sits in a tinted rounded square. */
  icon: ReactNode;
  tone?: RowTone;
  title: ReactNode;
  /** The setting's current value on one line ("Europe/Copenhagen"). Not a description. */
  value?: ReactNode;
}

/** The row is a link: the whole row opens `to`, a chevron shows on the right. */
interface LinkRow extends RowBase {
  to: string;
  /** Replaces the chevron (a small status word). */
  trailing?: never;
}
/** The row is a link to another site: it opens in a new tab and says so in its name. */
interface ExternalRow extends RowBase {
  href: string;
}
/** The row downloads a file from this server: a plain same-tab link the browser (or the app's web view) saves. */
interface DownloadRow extends RowBase {
  download: string;
}
/** The row is a button: the whole row runs `onClick`. */
interface ButtonRow extends RowBase {
  onClick: () => void;
  /** `chevron` (default) for "opens something", `none` for an action with no arrow. */
  chevron?: boolean;
  disabled?: boolean;
  /** Names the action when the title alone is not enough ("Log out of this account"). */
  label?: string;
}
/** The row is a switch: the whole row toggles it. */
interface SwitchRow extends RowBase {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
}
/** The row holds one small button at its right edge (the row itself is not interactive). */
interface ActionRow extends RowBase {
  action: ReactNode;
}
/** A fact: nothing to press. */
interface InfoRow extends RowBase {
  info: true;
}

export type SettingsRowProps = LinkRow | ExternalRow | DownloadRow | ButtonRow | SwitchRow | ActionRow | InfoRow;

function Body({ icon, tone = 'accent', title, value }: RowBase) {
  return (
    <>
      <span className="srow__icon" data-tone={tone} aria-hidden="true">
        {icon}
      </span>
      <span className="srow__text">
        <span className="srow__title">{title}</span>
        {value !== undefined && value !== null && value !== '' ? (
          <>
            {' '}
            <span className="srow__value">{value}</span>
          </>
        ) : null}
      </span>
    </>
  );
}

/**
 * One line of a settings list: icon, title, the current value, and what happens on a press.
 * The whole row is the control (a link, a button or a switch) so a thumb never has to aim at a
 * small target; its accessible name is the title followed by the value ("Timezone Europe/Copenhagen").
 * Put rows inside a `SettingsGroup`, which draws the card and the hairlines between them.
 */
export function SettingsRow(props: SettingsRowProps) {
  if ('to' in props) {
    return (
      <Link to={props.to} className="srow">
        <Body {...props} />
        <NextIcon size={18} />
      </Link>
    );
  }
  if ('download' in props) {
    return (
      <a href={props.download} className="srow" download>
        <Body {...props} />
        <NextIcon size={18} />
      </a>
    );
  }
  if ('href' in props) {
    return (
      <a href={props.href} className="srow" target="_blank" rel="noopener noreferrer">
        <Body {...props} />
        <NextIcon size={18} />
      </a>
    );
  }
  if ('onClick' in props) {
    return (
      <button type="button" className="srow" onClick={props.onClick} disabled={props.disabled} aria-label={props.label}>
        <Body {...props} />
        {props.chevron === false ? null : <NextIcon size={18} />}
      </button>
    );
  }
  if ('checked' in props) {
    return (
      <label className="srow">
        <Body {...props} />
        <input
          type="checkbox"
          role="switch"
          className="toggle__input"
          checked={props.checked}
          disabled={props.disabled}
          onChange={(e) => props.onCheckedChange(e.target.checked)}
        />
        <span className="toggle__track" aria-hidden="true" />
      </label>
    );
  }
  if ('action' in props) {
    return (
      <div className="srow srow--static">
        <Body {...props} />
        <span className="srow__action">{props.action}</span>
      </div>
    );
  }
  return (
    <div className="srow srow--static">
      <Body {...props} />
    </div>
  );
}

interface GroupProps {
  /** Small uppercase heading above the card ("Account"). */
  title: ReactNode;
  id?: string;
  children: ReactNode;
}

/** A heading and one card that holds rows separated by hairlines. Empty children render nothing. */
export function SettingsGroup({ title, id, children }: GroupProps) {
  const headingId = useId();
  const rows = Children.toArray(children);
  if (rows.length === 0) return null;
  return (
    <section className="sgroup" id={id} aria-labelledby={headingId}>
      <h2 className="sgroup__title" id={headingId}>
        {title}
      </h2>
      <ul className="sgroup__card">
        {rows.map((row, i) => (
          <li key={i}>{row}</li>
        ))}
      </ul>
    </section>
  );
}
