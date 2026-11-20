import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { api } from '../../api/client';
import type { DayResponse } from '../../api/generated/DayResponse';
import type { HrvRange } from '../../api/generated/HrvRange';
import type { SleepHistory } from '../../api/generated/SleepHistory';
import type { TrainingStatusRange } from '../../api/generated/TrainingStatusRange';
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

/** Training status day by day for local dates `from..=to` (GET /api/training/status). */
export function useTrainingStatus(from: string, to: string) {
  return useQuery({
    queryKey: ['training', 'status', from, to],
    queryFn: ({ signal }) => api.get<TrainingStatusRange>(`/api/training/status?from=${from}&to=${to}`, { signal }),
    placeholderData: keepPreviousData,
  });
}

/** HRV status day by day (GET /api/hrv). A one-day range also carries that night's readings. */
export function useHrv(from: string, to: string) {
  return useQuery({
    queryKey: ['hrv', from, to],
    queryFn: ({ signal }) => api.get<HrvRange>(`/api/hrv?from=${from}&to=${to}`, { signal }),
    placeholderData: keepPreviousData,
  });
}

export type SleepGroup = 'day' | 'week' | 'month';

/** Sleep score and time asleep, aggregated per day, week or month (GET /api/sleep/history). */
export function useSleepHistory(from: string, to: string, group: SleepGroup) {
  return useQuery({
    queryKey: ['sleep', 'history', from, to, group],
    queryFn: ({ signal }) => api.get<SleepHistory>(`/api/sleep/history?from=${from}&to=${to}&group=${group}`, { signal }),
    placeholderData: keepPreviousData,
  });
}
