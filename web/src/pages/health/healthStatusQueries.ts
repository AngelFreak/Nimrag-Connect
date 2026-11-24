import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { api } from '../../api/client';
import type { HealthStatusHistory } from '../../api/generated/HealthStatusHistory';
import type { HealthStatusResponse } from '../../api/generated/HealthStatusResponse';

/** Last night's overnight values against the usual range (GET /api/health-status). */
export function useHealthStatus() {
  return useQuery({
    queryKey: ['health-status'],
    queryFn: ({ signal }) => api.get<HealthStatusResponse>('/api/health-status', { signal }),
  });
}

/** The same for every night of local dates `from..=to` (GET /api/health-status/history). */
export function useHealthStatusHistory(from: string | null, to: string | null) {
  return useQuery({
    queryKey: ['health-status', 'history', from, to],
    queryFn: ({ signal }) => api.get<HealthStatusHistory>(`/api/health-status/history?from=${from}&to=${to}`, { signal }),
    enabled: from !== null && to !== null,
    placeholderData: keepPreviousData,
  });
}
