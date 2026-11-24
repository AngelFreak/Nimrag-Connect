import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '../../api/client';
import type { AssignRequest, AssignResponse, BackupSettings, BackupsResponse, NewUser, NewUserResponse, TemporaryPassword, UnassignedResponse, UserList } from '../../api/generated';

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

export const BACKUPS_KEY = ['admin', 'backups'] as const;
/** While a backup runs the page asks again this often to see it finish. */
const BACKUP_POLL_MS = 3000;

export function useBackups() {
  return useQuery({
    queryKey: BACKUPS_KEY,
    queryFn: ({ signal }) => api.get<BackupsResponse>('/api/admin/backups', { signal }),
    refetchInterval: (q) => (q.state.data?.status.running ? BACKUP_POLL_MS : false),
  });
}

/** Save, go back to the start-up options and Back up now all answer with the new state. */
function useBackupsMutation<V>(fn: (v: V) => Promise<BackupsResponse>) {
  const client = useQueryClient();
  return useMutation({ mutationFn: fn, onSuccess: (data) => client.setQueryData(BACKUPS_KEY, data) });
}

/** A refused value comes back as 422 with `{ error, field }` in the error's body. */
export function useSaveBackups() {
  return useBackupsMutation((body: BackupSettings) => api.put<BackupsResponse>('/api/admin/backups', body));
}

export function useUseStartupBackups() {
  return useBackupsMutation(() => api.delete<BackupsResponse>('/api/admin/backups'));
}

export function useRunBackup() {
  return useBackupsMutation(() => api.post<BackupsResponse>('/api/admin/backups/run'));
}

export function useAssign() {
  const refresh = useRefreshAdmin();
  return useMutation({ mutationFn: (body: AssignRequest) => api.post<AssignResponse>('/api/admin/unassigned/assign', body), onSuccess: refresh });
}
