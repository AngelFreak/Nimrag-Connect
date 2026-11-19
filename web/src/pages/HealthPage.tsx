import { HealthView } from './health/HealthView';

// Stage 3h: the Health hub. Trends from GET /api/trends?range=7d|4w|3m|1y and entry links to
// Sleep, Day, Movement, Training, Body and Calendar.

export function HealthPage() {
  return <HealthView />;
}
