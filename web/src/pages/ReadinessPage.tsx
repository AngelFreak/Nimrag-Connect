import { ReadinessView } from './health/ReadinessView';

// Nimrag's own readiness estimate (GET /api/readiness and /api/readiness/history).

export function ReadinessPage() {
  return <ReadinessView />;
}
