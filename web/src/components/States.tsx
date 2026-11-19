import type { CSSProperties, ReactNode } from 'react';
import { AlertIcon, InboxIcon } from './icons';

interface StateProps {
  title: string;
  children?: ReactNode;
  /** Optional action, usually a Button or link. */
  action?: ReactNode;
}

/** Nothing to show yet (no activities, no entries). Explains why and what to do. */
export function EmptyState({ title, children, action }: StateProps) {
  return (
    <div className="state">
      <span className="state__icon">
        <InboxIcon size={28} />
      </span>
      <p className="state__title">{title}</p>
      {children ? <p className="state__body">{children}</p> : null}
      {action}
    </div>
  );
}

/** A request failed. Announced to screen readers; `action` is typically a "Try again" button. */
export function ErrorState({ title, children, action }: StateProps) {
  return (
    <div className="state state--error" role="alert">
      <span className="state__icon">
        <AlertIcon size={28} />
      </span>
      <p className="state__title">{title}</p>
      {children ? <p className="state__body">{children}</p> : null}
      {action}
    </div>
  );
}

interface SkeletonProps {
  /** Height in px. */
  height?: number;
  /** CSS width ("60%", "120px"). */
  width?: string;
  /** Several stacked lines instead of one block. */
  lines?: number;
}

/** Loading placeholder. Hidden from assistive tech; the page announces loading with aria-busy. */
export function Skeleton({ height = 16, width = '100%', lines = 1 }: SkeletonProps) {
  if (lines > 1) {
    return (
      <div className="skeleton-stack" aria-hidden="true">
        {Array.from({ length: lines }, (_, i) => (
          <span key={i} className="skeleton" style={{ '--h': `${height}px`, '--w': i === lines - 1 ? '60%' : width } as CSSProperties} />
        ))}
      </div>
    );
  }
  return <span className="skeleton" aria-hidden="true" style={{ '--h': `${height}px`, '--w': width } as CSSProperties} />;
}
