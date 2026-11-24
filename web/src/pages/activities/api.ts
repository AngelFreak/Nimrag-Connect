// Queries and the strength mutation for the activity pages. Types come from the generated API types.

import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '../../api/client';
import { withShownCadence } from './cadence';
import type { ActivityDetail, ActivityList, StrengthPatch, StrengthResponse, TrackResponse } from '../../api/generated';

export const PAGE_SIZE = 30;

export function listPath(type: string, before: string | null): string {
  const q = new URLSearchParams();
  if (type !== 'all') q.set('type', type);
  q.set('limit', String(PAGE_SIZE));
  if (before) q.set('before', before);
  return `/api/activities?${q.toString()}`;
}

/** Newest first, "Load more" pages through `next_before`. */
export function useActivityList(type: string) {
  return useInfiniteQuery({
    queryKey: ['activities', 'list', type],
    initialPageParam: null as string | null,
    queryFn: ({ pageParam, signal }) => api.get<ActivityList>(listPath(type, pageParam), { signal }),
    getNextPageParam: (last) => last.next_before ?? undefined,
  });
}

export function useActivity(id: string | undefined) {
  return useQuery({
    queryKey: ['activities', 'detail', id],
    queryFn: ({ signal }) => api.get<ActivityDetail>(`/api/activities/${id}`, { signal }),
    // Cadence in the unit it is shown in (steps a minute on foot); the cache keeps the server's figures.
    select: withShownCadence,
    enabled: !!id,
  });
}

export function useTrack(id: string | undefined, enabled: boolean) {
  return useQuery({
    queryKey: ['activities', 'track', id],
    queryFn: ({ signal }) => api.get<TrackResponse>(`/api/activities/${id}/track`, { signal }),
    enabled: !!id && enabled,
  });
}

export function useStrength(id: string | undefined, enabled = true) {
  return useQuery({
    queryKey: ['activities', 'strength', id],
    queryFn: ({ signal }) => api.get<StrengthResponse>(`/api/activities/${id}/strength`, { signal }),
    enabled: !!id && enabled,
  });
}

/** PATCH the sets; the answer is the new session, which replaces the cached one. */
export function useSaveStrength(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (patch: StrengthPatch) => api.patch<StrengthResponse>(`/api/activities/${id}/strength`, patch),
    onSuccess: (fresh) => {
      client.setQueryData(['activities', 'strength', id], fresh);
      // Totals (sets, volume) in the list and on the detail page changed.
      void client.invalidateQueries({ queryKey: ['activities', 'list'] });
      void client.invalidateQueries({ queryKey: ['activities', 'detail', id] });
    },
  });
}
