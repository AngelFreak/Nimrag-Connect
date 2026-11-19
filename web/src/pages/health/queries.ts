import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { api } from '../../api/client';
import type { DayResponse } from '../../api/generated/DayResponse';
import type { SleepNightResponse } from '../../api/generated/SleepNightResponse';
import type { SleepNightsResponse } from '../../api/generated/SleepNightsResponse';
import type { TrendsResponse } from '../../api/generated/TrendsResponse';
import type { RangeKey } from './model';

export type NightCount = 7 | 14 | 30;

export function useSleepNight(date: string | null) {
  return useQuery({
    queryKey: ['sleep', 'night', date],
    queryFn: ({ signal }) => api.get<SleepNightResponse>(`/api/sleep/${date}`, { signal }),
    enabled: date !== null,
  });
}

export function useSleepNights(nights: NightCount) {
  return useQuery({
    queryKey: ['sleep', 'nights', nights],
    queryFn: ({ signal }) => api.get<SleepNightsResponse>(`/api/sleep?nights=${nights}`, { signal }),
    placeholderData: keepPreviousData,
  });
}

export function useDay(date: string) {
  return useQuery({
    queryKey: ['day', date],
    queryFn: ({ signal }) => api.get<DayResponse>(`/api/days/${date}`, { signal }),
  });
}

export function useTrends(range: RangeKey) {
  return useQuery({
    queryKey: ['trends', range],
    queryFn: ({ signal }) => api.get<TrendsResponse>(`/api/trends?range=${range}`, { signal }),
    placeholderData: keepPreviousData,
  });
}
