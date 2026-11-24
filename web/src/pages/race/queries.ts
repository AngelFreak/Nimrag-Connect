import { useQuery } from '@tanstack/react-query';
import { api } from '../../api/client';
import type { RacePredictionsResponse } from '../../api/generated/RacePredictionsResponse';

/** Nimrag's race time estimates and their weekly history (GET /api/race-predictions). */
export function useRacePredictions() {
  return useQuery({
    queryKey: ['race-predictions'],
    queryFn: ({ signal }) => api.get<RacePredictionsResponse>('/api/race-predictions', { signal }),
  });
}
