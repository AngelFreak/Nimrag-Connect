import type { ReactNode } from 'react';
import { statusColor, type StatusType } from '../lib/status';

interface StatusWordProps {
  /** Which family of statuses the key belongs to. */
  type: StatusType;
  /** The status key: "productive", "balanced", "good", "high", "rest". Unknown keys draw no dot. */
  value: string | null | undefined;
  /** The word as it is shown. */
  children: ReactNode;
  size?: 'md' | 'lg';
}

/**
 * A status as a small filled dot in its colour followed by its word. The dot is decoration; the word
 * carries the meaning, so the pair works without colour.
 */
export function StatusWord({ type, value, children, size = 'md' }: StatusWordProps) {
  const color = statusColor(type, value);
  return (
    <span className={size === 'lg' ? 'status-word status-word--lg' : 'status-word'}>
      {color ? <span className="status-word__dot" aria-hidden="true" style={{ background: color }} /> : null}
      <span className="status-word__text">{children}</span>
    </span>
  );
}
