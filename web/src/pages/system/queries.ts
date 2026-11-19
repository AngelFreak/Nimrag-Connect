import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '../../api/client';
import type {
  GarminBackfillRequest,
  GarminBackfillResponse,
  GarminImportTokens,
  GarminLoginRequest,
  GarminLoginResponse,
  GarminMfaRequest,
  GarminStatus,
  NewToken,
  NewTokenResponse,
  SettingsPatch,
  SettingsResponse,
  TokenList,
} from '../../api/generated';
import { SETTINGS_KEY } from '../today/queries';

export const TOKENS_KEY = ['tokens'] as const;
export const GARMIN_KEY = ['garmin'] as const;

export function useTokens() {
  return useQuery({ queryKey: TOKENS_KEY, queryFn: ({ signal }) => api.get<TokenList>('/api/tokens', { signal }) });
}

/** Creates a device token. The secret is returned to the caller only; it is never put in the query cache. */
export function useCreateToken() {
  const client = useQueryClient();
  return useMutation({
    // A wrong owner password is 403 (not 401), so it is not mistaken for a lost session.
    mutationFn: (body: NewToken) => api.post<NewTokenResponse>('/api/tokens', body),
    onSuccess: () => client.invalidateQueries({ queryKey: TOKENS_KEY }),
  });
}

export function useRevokeToken() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => api.delete<void>(`/api/tokens/${id}`),
    onSuccess: () => client.invalidateQueries({ queryKey: TOKENS_KEY }),
  });
}

export function useSaveSettings() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (patch: SettingsPatch) => api.patch<SettingsResponse>('/api/settings', patch),
    onSuccess: (saved) => {
      client.setQueryData(SETTINGS_KEY, saved);
      // Days are local days in this timezone and goals feed several pages: refetch what depends on them.
      void client.invalidateQueries({ predicate: (q) => q.queryKey[0] !== 'settings' });
    },
  });
}

export function useGarmin() {
  return useQuery({ queryKey: GARMIN_KEY, queryFn: ({ signal }) => api.get<GarminStatus>('/api/garmin', { signal }) });
}

/** The Forward actions. Each one ends by refreshing the status. */
export function useGarminActions() {
  const client = useQueryClient();
  const refresh = () => client.invalidateQueries({ queryKey: GARMIN_KEY });
  const setStatus = (s: GarminStatus) => client.setQueryData(GARMIN_KEY, s);
  return {
    login: useMutation({
      mutationFn: (body: GarminLoginRequest) => api.post<GarminLoginResponse>('/api/garmin/login', body),
      onSuccess: refresh,
    }),
    mfa: useMutation({
      mutationFn: (body: GarminMfaRequest) => api.post<GarminStatus>('/api/garmin/mfa', body),
      onSuccess: setStatus,
    }),
    importTokens: useMutation({
      mutationFn: (body: GarminImportTokens) => api.post<GarminStatus>('/api/garmin/import-tokens', body),
      onSuccess: setStatus,
    }),
    retry: useMutation({ mutationFn: () => api.post<GarminStatus>('/api/garmin/retry'), onSuccess: setStatus }),
    backfill: useMutation({
      mutationFn: (body: GarminBackfillRequest) => api.post<GarminBackfillResponse>('/api/garmin/backfill', body),
      onSuccess: refresh,
    }),
    logout: useMutation({ mutationFn: () => api.post<void>('/api/garmin/logout'), onSuccess: refresh }),
  };
}
