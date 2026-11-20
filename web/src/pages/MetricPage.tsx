import { useParams } from 'react-router-dom';
import { MetricView } from './metrics/MetricView';
import { isMetricKey } from './metrics/period';
import { NotFoundPage } from './NotFoundPage';

// A metric's detail page (/metrics/steps): the 1-day view from GET /api/days/{date}, longer
// periods from GET /api/metrics/{metric}.

export function MetricPage() {
  const { metric } = useParams();
  return isMetricKey(metric) ? <MetricView metric={metric} /> : <NotFoundPage />;
}
