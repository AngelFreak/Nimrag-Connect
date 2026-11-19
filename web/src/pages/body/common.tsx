import { useQuery } from '@tanstack/react-query';
import { api, ApiError } from '../../api/client';
import type { Today } from '../../api/generated';
import { Button, Card, ErrorState, Skeleton } from '../../components';
import { browserToday, isIsoDay } from '../../lib/dates';

/**
 * The owner's "today" (their timezone, decided by the server). `enabled: false` skips the
 * request when the page already has a date; until the answer arrives `undefined` is returned.
 * If the request fails the browser's own date is the fallback.
 */
export function useOwnerToday(enabled = true): string | undefined {
  const q = useQuery({
    queryKey: ['owner-today'],
    queryFn: ({ signal }) => api.get<Today>('/api/today', { signal }),
    enabled,
    select: (t) => t.date,
  });
  if (!enabled) return undefined;
  if (q.data && isIsoDay(q.data)) return q.data;
  if (q.isError) return browserToday();
  return undefined;
}

/** Loading placeholder for a page body: announced with aria-busy, the skeleton itself is hidden from assistive tech. */
export function PageSkeleton({ cards = 3 }: { cards?: number }) {
  return (
    <div className="page" aria-busy="true">
      <p className="sr-only" role="status">
        Loading
      </p>
      {Array.from({ length: cards }, (_, i) => (
        <Card key={i} padding="lg">
          <Skeleton height={14} width="40%" />
          <Skeleton height={32} width="55%" />
          <Skeleton height={64} />
        </Card>
      ))}
    </div>
  );
}

export function errorText(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  return 'The server could not be reached. Check the connection and try again.';
}

/** Failed request: says what went wrong and offers a retry. */
export function PageError({ title, error, onRetry }: { title: string; error: unknown; onRetry: () => void }) {
  return (
    <div className="page">
      <ErrorState
        title={title}
        action={
          <Button variant="primary" onClick={onRetry}>
            Try again
          </Button>
        }
      >
        {errorText(error)}
      </ErrorState>
    </div>
  );
}
