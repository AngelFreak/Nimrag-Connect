import type { ReactNode } from 'react';
import { AlertIcon } from './icons';

interface NoticeProps {
  tone?: 'info' | 'warn';
  children: ReactNode;
}

/**
 * A short boxed note inside a card, for what a person should know before acting (the unofficial
 * Garmin login). Not an alert: nothing went wrong, so it is not announced.
 */
export function Notice({ tone = 'info', children }: NoticeProps) {
  return (
    <div className={tone === 'warn' ? 'notice notice--warn' : 'notice'}>
      {tone === 'warn' ? <AlertIcon size={18} /> : null}
      <p>{children}</p>
    </div>
  );
}
