import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { UnauthorizedError, api, apiFetch, clearOfflineApiCache } from '../api/client';
import type { Me, SetupStatus } from '../api/generated';

export const ME_KEY = ['me'] as const;
export const SETUP_KEY = ['setup'] as const;

function isMe(value: unknown): value is Me {
  return typeof value === 'object' && value !== null && 'username' in value && 'role' in value;
}

/**
 * GET /api/me: who is signed in. `null` means nobody (401). Anything that is not a `Me`
 * (a static host answering with its index page) counts as nobody too. `enabled: false` only
 * reads what is cached (the component gallery shows a profile button without a server).
 */
export function useMe(enabled = true) {
  return useQuery({
    queryKey: ME_KEY,
    enabled,
    queryFn: async ({ signal }): Promise<Me | null> => {
      try {
        const me = await apiFetch<unknown>('/api/me', { signal, redirectOn401: false });
        return isMe(me) ? me : null;
      } catch (err) {
        if (err instanceof UnauthorizedError) return null;
        throw err;
      }
    },
    staleTime: 60_000,
  });
}

/** GET /api/setup: whether the server still waits for its first admin. Needs no session. */
export function useSetupStatus() {
  return useQuery({
    queryKey: SETUP_KEY,
    queryFn: ({ signal }) => api.get<SetupStatus>('/api/setup', { signal }),
    staleTime: 0,
  });
}

/** The login and setup answers carry a `step` tag next to the `Me` fields; the cache holds a plain `Me`. */
export function toMe(answer: Me): Me {
  return { authenticated: answer.authenticated, username: answer.username, role: answer.role, timezone: answer.timezone, sample: answer.sample };
}

/** Where a person lands after signing in: admins in /admin, users where they were going. */
export function landingPath(me: Me, next: string | null): string {
  const target = next && next.startsWith('/') && !next.startsWith('//') && !next.startsWith('/login') && !next.startsWith('/setup') ? next : null;
  if (me.role === 'admin') return target?.startsWith('/admin') ? target : '/admin';
  return target && !target.startsWith('/admin') ? target : '/';
}

/** Stores the new session's `Me` and drops everything the previous person had cached. */
export function useStartSession() {
  const queryClient = useQueryClient();
  return (me: Me) => {
    queryClient.clear();
    queryClient.setQueryData(ME_KEY, toMe(me));
  };
}

/** POST /api/logout, then forget everything this browser cached for the person. */
export function useLogout() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  return useMutation({
    mutationFn: async () => {
      try {
        await api.post('/api/logout', undefined, { redirectOn401: false });
      } catch {
        // Already logged out (401) or the server is unreachable: leave the page either way.
      }
      await clearOfflineApiCache();
      queryClient.clear();
    },
    onSettled: () => {
      void navigate('/login', { replace: true });
    },
  });
}
