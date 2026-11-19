import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '../../api/client';
import type {
  ActivityList,
  DevicesResponse,
  HomeCards,
  SettingsPatch,
  SettingsResponse,
  SleepNightsResponse,
  StorageResponse,
  Today,
  TrendsResponse,
} from '../../api/generated';

export const TODAY_KEY = ['today'] as const;
export const SETTINGS_KEY = ['settings'] as const;

export function useToday() {
  return useQuery({ queryKey: TODAY_KEY, queryFn: ({ signal }) => api.get<Today>('/api/today', { signal }) });
}

export function useSettings() {
  return useQuery({ queryKey: SETTINGS_KEY, queryFn: ({ signal }) => api.get<SettingsResponse>('/api/settings', { signal }) });
}

/** Desktop overview: the five newest activities. */
export function useRecentActivities(enabled: boolean) {
  return useQuery({
    queryKey: ['overview', 'activities', 5],
    queryFn: ({ signal }) => api.get<ActivityList>('/api/activities?limit=5', { signal }),
    enabled,
  });
}

export type TrendRange = '7d' | '4w' | '3m' | '1y';

export function useTrends(range: TrendRange, enabled: boolean) {
  return useQuery({
    queryKey: ['overview', 'trends', range],
    queryFn: ({ signal }) => api.get<TrendsResponse>(`/api/trends?range=${range}`, { signal }),
    enabled,
  });
}

export function useSleepNights(nights: 7 | 14 | 30, enabled: boolean) {
  return useQuery({
    queryKey: ['overview', 'sleep-nights', nights],
    queryFn: ({ signal }) => api.get<SleepNightsResponse>(`/api/sleep?nights=${nights}`, { signal }),
    enabled,
  });
}

export function useStorage(enabled: boolean) {
  return useQuery({ queryKey: ['overview', 'storage'], queryFn: ({ signal }) => api.get<StorageResponse>('/api/storage', { signal }), enabled });
}

export function useDevices(enabled: boolean) {
  return useQuery({ queryKey: ['overview', 'devices'], queryFn: ({ signal }) => api.get<DevicesResponse>('/api/devices', { signal }), enabled });
}

/**
 * Saves the card layout. The cache is updated first (optimistic); when the server refuses,
 * the previous layout is put back and the caller gets the error to show.
 */
export function useSaveHomeCards() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (home_cards: HomeCards) => api.patch<SettingsResponse>('/api/settings', { home_cards } satisfies SettingsPatch),
    onMutate: async (home_cards) => {
      await queryClient.cancelQueries({ queryKey: SETTINGS_KEY });
      const previous = queryClient.getQueryData<SettingsResponse>(SETTINGS_KEY);
      if (previous) queryClient.setQueryData<SettingsResponse>(SETTINGS_KEY, { ...previous, home_cards });
      return { previous };
    },
    onError: (_error, _cards, context) => {
      if (context?.previous) queryClient.setQueryData(SETTINGS_KEY, context.previous);
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(SETTINGS_KEY, saved);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: SETTINGS_KEY }),
  });
}
