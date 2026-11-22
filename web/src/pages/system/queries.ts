import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from '../../api/client';
import { ME_KEY, toMe } from '../../account/queries';
import type {
  AboutResponse,
  ChangeOwnPassword,
  GarminBackfillRequest,
  GarminBackfillResponse,
  GarminImportTokens,
  GarminLoginRequest,
  GarminLoginResponse,
  GarminMfaRequest,
  GarminSettingsRequest,
  GarminStatus,
  Me,
  SettingsPatch,
  SettingsResponse,
  SetupStartResponse,
  TokenList,
  TwoFactorConfirm,
  TwoFactorStart,
} from '../../api/generated';
import { SETTINGS_KEY } from '../today/queries';

export const TOKENS_KEY = ['tokens'] as const;
export const GARMIN_KEY = ['garmin'] as const;
export const ABOUT_KEY = ['about'] as const;

/** GET /api/about: the server's version. Any signed-in session may ask, admins included. */
export function useAbout() {
  return useQuery({ queryKey: ABOUT_KEY, queryFn: ({ signal }) => api.get<AboutResponse>('/api/about', { signal }), staleTime: 5 * 60_000 });
}

export function useTokens(enabled = true) {
  return useQuery({ queryKey: TOKENS_KEY, queryFn: ({ signal }) => api.get<TokenList>('/api/tokens', { signal }), enabled });
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

export function useGarmin(enabled = true) {
  return useQuery({ queryKey: GARMIN_KEY, queryFn: ({ signal }) => api.get<GarminStatus>('/api/garmin', { signal }), enabled });
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
    /** Activities only, or everything from the watch. Queues nothing by itself. */
    chooseWhatToSend: useMutation({
      mutationFn: (body: GarminSettingsRequest) => api.post<GarminStatus>('/api/garmin/settings', body),
      onSuccess: setStatus,
    }),
    logout: useMutation({ mutationFn: () => api.post<void>('/api/garmin/logout'), onSuccess: refresh }),
  };
}

/** POST /api/me/password: a new session replaces this one (the others end), so the cached `Me` is refreshed. */
export function useChangePassword() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: ChangeOwnPassword) => api.post<Me>('/api/me/password', body),
    onSuccess: (me) => client.setQueryData(ME_KEY, toMe(me)),
  });
}

/** Re-enrolling two-factor: the password first (403 when wrong), then the QR code, then a code from it. */
export function useStartTwoFactor() {
  return useMutation({ mutationFn: (body: TwoFactorStart) => api.post<SetupStartResponse>('/api/me/2fa', body) });
}

/** 401 here means the pending step expired, not a lost session, so it must not send the page to /login. */
export function useConfirmTwoFactor() {
  return useMutation({ mutationFn: (body: TwoFactorConfirm) => api.post<void>('/api/me/2fa/confirm', body, { redirectOn401: false }) });
}
