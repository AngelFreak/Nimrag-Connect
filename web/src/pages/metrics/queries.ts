import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { api } from '../../api/client';
import type { MetricRange } from '../../api/generated/MetricRange';
import type { MovementResponse } from '../../api/generated/MovementResponse';
import type { MetricKey, ServerRange } from './period';

/** One period of a metric (GET /api/metrics/{metric}), ending on `end`. The last period stays on screen while the next loads. */
export function useMetricRange(metric: MetricKey, range: ServerRange | null, end: string) {
  return useQuery({
    queryKey: ['metric', metric, range, end],
    queryFn: ({ signal }) => api.get<MetricRange>(`/api/metrics/${metric}?range=${range}&end=${end}`, { signal }),
    enabled: range !== null,
    placeholderData: keepPreviousData,
  });
}

/** Distance, calories and the week around a day (GET /api/movement/{date}); the same key as the Movement page. */
export function useMovementDay(date: string, enabled: boolean) {
  return useQuery({
    queryKey: ['movement', date],
    queryFn: ({ signal }) => api.get<MovementResponse>(`/api/movement/${date}`, { signal }),
    enabled,
  });
}
