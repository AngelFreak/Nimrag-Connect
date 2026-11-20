import { HrvView } from './health/HrvView';

// The HRV status page: 7-day averages against the balanced range, and one night (GET /api/hrv).

export function HrvPage() {
  return <HrvView />;
}
