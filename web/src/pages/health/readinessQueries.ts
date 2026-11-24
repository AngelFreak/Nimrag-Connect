import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { api } from '../../api/client';
import type { ReadinessHistory } from '../../api/generated/ReadinessHistory';
import type { ReadinessResponse } from '../../api/generated/ReadinessResponse';
import type { StatusExplain } from '../../api/generated/StatusExplain';

/** Nimrag's readiness estimate for today (GET /api/readiness). */
export function useReadiness() {
  return useQuery({
    queryKey: ['readiness'],
    queryFn: ({ signal }) => api.get<ReadinessResponse>('/api/readiness', { signal }),
  });
}

/** The estimate day by day for local dates `from..=to` (GET /api/readiness/history). */
export function useReadinessHistory(from: string, to: string) {
  return useQuery({
    queryKey: ['readiness', 'history', from, to],
    queryFn: ({ signal }) => api.get<ReadinessHistory>(`/api/readiness/history?from=${from}&to=${to}`, { signal }),
    placeholderData: keepPreviousData,
  });
}

/** Why the training status is what it is (GET /api/training/explain). */
export function useStatusExplain() {
  return useQuery({
    queryKey: ['training', 'explain'],
    queryFn: ({ signal }) => api.get<StatusExplain>('/api/training/explain', { signal }),
  });
}
