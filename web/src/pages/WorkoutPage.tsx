import { Card, CardHeader, LinkButton, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';

// The workout builder (writing workouts to the watch) is stage 4a. Until then this page says so
// and shows no sample steps, so nothing here looks saved or sent.

export function WorkoutPage() {
  usePageTitle('New workout');
  return (
    <>
      <TopBar variant="back" backTo="/activities?type=strength" backLabel="Back to strength sessions" title="Workout" subtitle="Coming in stage 4a" />
      <div className="page">
        <Card padding="lg">
          <CardHeader title="Workout builder" tone="muted" />
          <p style={{ margin: 0, fontSize: 'var(--fs-lg)', fontWeight: 600 }}>Not available yet</p>
          <p style={{ margin: 0, fontSize: 'var(--fs-base)', color: 'var(--text-2)' }}>
            Building a workout and sending it to the watch comes in stage 4a. Nothing can be created or sent from here yet, so no steps are shown.
          </p>
          <p style={{ margin: 0, fontSize: 'var(--fs-base)', color: 'var(--text-2)' }}>Your recorded strength sessions, with their sets, are under Strength.</p>
        </Card>
        <LinkButton to="/activities?type=strength" size="lg" variant="primary">
          Go to strength sessions
        </LinkButton>
      </div>
    </>
  );
}
