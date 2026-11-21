import { useParams } from 'react-router-dom';
import { ApiError } from '../api/client';
import { EmptyState, LinkButton, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { PageError, PageSkeleton } from './body/common';
import { useWorkout, useWorkoutList } from './workouts/api';
import { WorkoutEditor } from './workouts/WorkoutEditor';
import './workouts/workouts.css';

/**
 * The builder at `/workouts/new` (a new workout) and `/workouts/:id` (one that is saved). The
 * list query supplies the limits and whether this server can send to the watch; the editor is
 * created once both are here, so what the person types is never replaced by a refresh.
 */
export function WorkoutPage() {
  const { id } = useParams();
  const parsed = id === undefined ? null : /^\d+$/.test(id) ? Number(id) : Number.NaN;
  usePageTitle(parsed === null ? 'New workout' : 'Workout');
  const list = useWorkoutList();
  const detail = useWorkout(Number.isNaN(parsed) ? null : parsed);

  const missing = Number.isNaN(parsed) || (detail.error instanceof ApiError && detail.error.status === 404);
  if (missing) {
    return (
      <>
        <TopBar variant="back" backTo="/workouts" backLabel="Back to workouts" title="Workout" />
        <div className="page">
          <EmptyState
            title="This workout was not found"
            action={
              <LinkButton to="/workouts" variant="primary">
                Go to workouts
              </LinkButton>
            }
          >
            It may have been deleted.
          </EmptyState>
        </div>
      </>
    );
  }
  if (list.isError && !list.data) return <FailedPage error={list.error} onRetry={() => void list.refetch()} />;
  if (parsed !== null && detail.isError && !detail.data) return <FailedPage error={detail.error} onRetry={() => void detail.refetch()} />;
  if (!list.data || (parsed !== null && !detail.data)) {
    return (
      <>
        <TopBar variant="back" backTo="/workouts" backLabel="Back to workouts" title={parsed === null ? 'New workout' : 'Workout'} />
        <PageSkeleton cards={3} />
      </>
    );
  }
  return <WorkoutEditor key={parsed ?? 'new'} detail={detail.data ?? null} list={list.data} />;
}

function FailedPage({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  return (
    <>
      <TopBar variant="back" backTo="/workouts" backLabel="Back to workouts" title="Workout" />
      <PageError title="The workout could not be loaded" error={error} onRetry={onRetry} />
    </>
  );
}

