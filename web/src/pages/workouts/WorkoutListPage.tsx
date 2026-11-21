import { Button, EmptyState, LinkButton, ListRow, StatusBanner, TopBar } from '../../components';
import { usePageTitle } from '../../lib/hooks';
import { PageError, PageSkeleton } from '../body/common';
import { useWorkoutList } from './api';
import { sendLabel } from './send';
import { sportLabel, totalText } from './model';
import './workouts.css';

/** The saved workouts, newest change first, each with where it is on its way to the watch. */
export function WorkoutListPage() {
  usePageTitle('Workouts');
  const query = useWorkoutList();
  const data = query.data;

  return (
    <>
      <TopBar variant="back" backTo="/strength" backLabel="Back to strength sessions" title="Workouts" subtitle="Build one here, send it to the watch" />
      {query.isError && !data ? (
        <PageError title="Workouts could not be loaded" error={query.error} onRetry={() => void query.refetch()} />
      ) : !data ? (
        <PageSkeleton cards={2} />
      ) : (
        <>
          <div className="page">
            {!data.watch_send.available ? (
              <StatusBanner tone="info">{data.watch_send.reason ?? 'This server cannot send workouts to the watch.'}</StatusBanner>
            ) : null}
            {data.workouts.length === 0 ? (
              <EmptyState
                title="No workouts yet"
                action={
                  <LinkButton to="/workouts/new" variant="primary">
                    New workout
                  </LinkButton>
                }
              >
                Build a workout with warm-up, intervals and targets, save it here and send it to the watch.
              </EmptyState>
            ) : (
              <ul className="wk-steps wk-list" aria-label="Workouts">
                {data.workouts.map((w) => {
                  const total = totalText(w.duration_s);
                  return (
                    <li key={w.id}>
                      <ListRow
                        to={`/workouts/${w.id}`}
                        title={w.name}
                        sub={[sportLabel(w.sport), `${w.steps} ${w.steps === 1 ? 'step' : 'steps'}`, total].filter(Boolean).join(' · ')}
                        value={sendLabel(w.send)}
                        valueSub={w.send.state === 'failed' ? 'Open it to see why' : undefined}
                      />
                    </li>
                  );
                })}
              </ul>
            )}
            {query.isError ? (
              <p className="status-msg status-msg--error" role="alert">
                The list could not be refreshed. <Button onClick={() => void query.refetch()}>Try again</Button>
              </p>
            ) : null}
          </div>
          <div className="footer-bar footer-bar--single">
            <LinkButton to="/workouts/new" size="lg" variant="primary" block>
              New workout
            </LinkButton>
          </div>
        </>
      )}
    </>
  );
}
