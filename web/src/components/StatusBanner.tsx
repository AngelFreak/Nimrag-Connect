import type { ReactNode } from 'react';
import { AlertIcon, InfoIcon } from './icons';

interface StatusBannerProps {
  /** `warn` for a problem that needs a fix, `info` for a heads-up. */
  tone?: 'warn' | 'info';
  /** The whole message: one line, what is wrong ("Garmin asks you to log in again."). */
  children: ReactNode;
  /** The one fix, usually a small `Button` or `LinkButton`. Leave it out when there is none. */
  action?: ReactNode;
}

/**
 * A problem as one line and one button that fixes it; the page drops the banner as soon as the
 * problem is gone. It is not a live region: nothing moved while someone was looking at it.
 */
export function StatusBanner({ tone = 'warn', children, action }: StatusBannerProps) {
  return (
    <div className="sbanner" data-tone={tone}>
      <span className="sbanner__icon" aria-hidden="true">
        {tone === 'warn' ? <AlertIcon size={20} /> : <InfoIcon size={20} />}
      </span>
      <p className="sbanner__text">{children}</p>
      {action ? <span className="sbanner__action">{action}</span> : null}
    </div>
  );
}
