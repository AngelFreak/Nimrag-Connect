import { ApiError, NetworkError } from '../../api/client';
import { Button, Card, ErrorState, LinkButton, Skeleton } from '../../components';

/** Loading placeholder for a whole page body; announces itself with aria-busy. */
export function PageSkeleton({ label, tiles = 6 }: { label: string; tiles?: number }) {
  return (
    <div className="page" aria-busy="true" aria-live="polite">
      <span className="sr-only">{label}</span>
      <Skeleton height={150} />
      <div className="grid-3">
        {Array.from({ length: tiles }, (_, i) => (
          <Skeleton key={i} height={64} />
        ))}
      </div>
      <Skeleton height={14} lines={4} />
    </div>
  );
}

/** A list of skeleton rows. */
export function RowsSkeleton() {
  return (
    <div className="page" style={{ paddingTop: 0 }} aria-busy="true" aria-live="polite">
      <span className="sr-only">Loading activities</span>
      {Array.from({ length: 6 }, (_, i) => (
        <Skeleton key={i} height={68} />
      ))}
    </div>
  );
}

export function errorText(error: unknown): string {
  if (error instanceof NetworkError) return 'Could not reach the server. Check the connection and try again.';
  if (error instanceof ApiError) return error.message;
  return 'Something went wrong.';
}

/** Failed request: says what failed, offers a retry; a 404 gets its own wording and a way back. */
export function QueryError({ error, what, onRetry, backTo }: { error: unknown; what: string; onRetry: () => void; backTo: string }) {
  if (error instanceof ApiError && error.status === 404) {
    return (
      <div className="page">
        <ErrorState title={`${what} not found`} action={<LinkButton to={backTo}>Back to activities</LinkButton>}>
          It may have been removed, or the link is wrong.
        </ErrorState>
      </div>
    );
  }
  return (
    <div className="page">
      <Card padding="lg">
        <ErrorState title={`Could not load ${what.toLowerCase()}`} action={<Button onClick={onRetry}>Try again</Button>}>
          {errorText(error)}
        </ErrorState>
      </Card>
    </div>
  );
}
