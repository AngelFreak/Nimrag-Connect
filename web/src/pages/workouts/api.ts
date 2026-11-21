import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '../../api/client';
import type { WorkoutBody, WorkoutDetail, WorkoutList } from '../../api/generated';

// GET/POST /api/workouts, GET/PUT/DELETE /api/workouts/{id}, POST/DELETE /api/workouts/{id}/send.

/** How often a queued workout is looked at again: the phone reports back in the background. */
export const QUEUED_REFRESH_MS = 5000;

export const LIST_KEY = ['workouts'] as const;
export const detailKey = (id: number) => ['workouts', id] as const;

export function useWorkoutList() {
  return useQuery({
    queryKey: LIST_KEY,
    queryFn: ({ signal }) => api.get<WorkoutList>('/api/workouts', { signal }),
    // While something waits for the phone, the list shows when it was sent.
    refetchInterval: (q) => (q.state.data?.workouts.some((w) => w.send.state === 'queued') ? QUEUED_REFRESH_MS : false),
  });
}

export function useWorkout(id: number | null) {
  return useQuery({
    queryKey: detailKey(id ?? 0),
    enabled: id !== null,
    queryFn: ({ signal }) => api.get<WorkoutDetail>(`/api/workouts/${id}`, { signal }),
    refetchInterval: (q) => (q.state.data?.send.state === 'queued' ? QUEUED_REFRESH_MS : false),
  });
}

/** What every change does to the cache: the list is stale, and the changed workout is replaced by the answer. */
function useAfterChange() {
  const client = useQueryClient();
  return async (detail?: WorkoutDetail) => {
    if (detail) client.setQueryData(detailKey(detail.id), detail);
    await client.invalidateQueries({ queryKey: LIST_KEY, exact: true });
  };
}

export function useSaveWorkout(id: number | null) {
  const after = useAfterChange();
  return useMutation({
    mutationFn: (body: WorkoutBody) => (id === null ? api.post<WorkoutDetail>('/api/workouts', body) : api.put<WorkoutDetail>(`/api/workouts/${id}`, body)),
    onSuccess: after,
  });
}

export function useDeleteWorkout() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => api.delete<void>(`/api/workouts/${id}`),
    onSuccess: async (_data, id) => {
      client.removeQueries({ queryKey: detailKey(id) });
      await client.invalidateQueries({ queryKey: LIST_KEY, exact: true });
    },
  });
}

export function useSendWorkout() {
  const after = useAfterChange();
  return useMutation({
    mutationFn: ({ id, send }: { id: number; send: boolean }) =>
      send ? api.post<WorkoutDetail>(`/api/workouts/${id}/send`) : api.delete<WorkoutDetail>(`/api/workouts/${id}/send`),
    onSuccess: after,
  });
}
