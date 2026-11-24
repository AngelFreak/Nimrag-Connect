import { HealthStatusView } from './health/HealthStatusView';

// Overnight values against your own usual range (GET /api/health-status and its history).

export function HealthStatusPage() {
  return <HealthStatusView />;
}
