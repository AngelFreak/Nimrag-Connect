import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '../../api/client';
import type { AssignRequest, AssignResponse, NewUser, NewUserResponse, TemporaryPassword, UnassignedResponse, UserList } from '../../api/generated';

export const ADMIN_KEY = ['admin'] as const;
export const USERS_KEY = ['admin', 'users'] as const;
export const UNASSIGNED_KEY = ['admin', 'unassigned'] as const;

export function useUsers() {
  return useQuery({ queryKey: USERS_KEY, queryFn: ({ signal }) => api.get<UserList>('/api/admin/users', { signal }) });
}

export function useUnassigned() {
  return useQuery({ queryKey: UNASSIGNED_KEY, queryFn: ({ signal }) => api.get<UnassignedResponse>('/api/admin/unassigned', { signal }) });
}

/** Every change can alter both lists (a deleted user's tokens come back as unassigned), so both refresh. */
function useRefreshAdmin() {
  const client = useQueryClient();
  return () => client.invalidateQueries({ queryKey: ADMIN_KEY });
}

/** The temporary password comes back to the caller only; it is not kept in the query cache. */
export function useCreateUser() {
  const refresh = useRefreshAdmin();
  return useMutation({ mutationFn: (body: NewUser) => api.post<NewUserResponse>('/api/admin/users', body), onSuccess: refresh });
}

export function useResetPassword() {
  const refresh = useRefreshAdmin();
  return useMutation({ mutationFn: (id: number) => api.post<TemporaryPassword>(`/api/admin/users/${id}/reset-password`), onSuccess: refresh });
}

export function useResetTwoFactor() {
  const refresh = useRefreshAdmin();
  return useMutation({ mutationFn: (id: number) => api.post<void>(`/api/admin/users/${id}/reset-2fa`), onSuccess: refresh });
}

export function useSetDisabled() {
  const refresh = useRefreshAdmin();
  return useMutation({
    mutationFn: ({ id, disabled }: { id: number; disabled: boolean }) => api.post<void>(`/api/admin/users/${id}/${disabled ? 'disable' : 'enable'}`),
    onSuccess: refresh,
  });
}

/** Without `confirm` the server answers 409 when the user has data. */
export function useDeleteUser() {
  const refresh = useRefreshAdmin();
  return useMutation({
    mutationFn: ({ id, confirm }: { id: number; confirm: boolean }) => api.delete<void>(`/api/admin/users/${id}${confirm ? '?confirm=true' : ''}`),
    onSuccess: refresh,
  });
}

export function useAssign() {
  const refresh = useRefreshAdmin();
  return useMutation({ mutationFn: (body: AssignRequest) => api.post<AssignResponse>('/api/admin/unassigned/assign', body), onSuccess: refresh });
}
