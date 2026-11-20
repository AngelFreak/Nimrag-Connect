import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { Button, Card, ErrorState, Skeleton } from '../../components';
import './health.css';

/**
 * One-line text summary of a chart. Charts from SeriesChart already carry the summary for screen
 * readers, so the visible copy is hidden from them (`hidden`) to avoid reading it twice.
 */
export function ChartSummary({ children, hidden }: { children: ReactNode; hidden?: boolean }) {
  return (
    <p className="chart-summary" aria-hidden={hidden ? true : undefined}>
      {children}
    </p>
  );
}

export function LoadError({ what, error, onRetry }: { what: string; error: unknown; onRetry: () => void }) {
  const message = error instanceof Error ? error.message : undefined;
  return (
    <ErrorState title={`Could not load ${what}`} action={<Button onClick={onRetry}>Try again</Button>}>
      {message}
    </ErrorState>
  );
}

/** Loading placeholder with the rough shape of a chart card. */
export function CardSkeleton({ lines = 3, height = 16 }: { lines?: number; height?: number }) {
  return (
    <Card>
      <Skeleton height={height} width="40%" />
      <Skeleton height={height} lines={lines} />
    </Card>
  );
}

/** A card-sized link used for the entry points on the Health hub. */
export function HubLink({ to, title, sub }: { to: string; title: string; sub: string }) {
  return (
    <Link to={to} className="list-row">
      <span className="list-row__main">
        <span className="list-row__title">{title}</span>
        <span className="list-row__sub">{sub}</span>
      </span>
      <span aria-hidden="true" style={{ color: 'var(--text-2)', fontSize: 22 }}>
        ›
      </span>
    </Link>
  );
}

