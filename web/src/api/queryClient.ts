import { QueryClient } from '@tanstack/react-query';
import { ApiError, NetworkError } from './client';

/** 4xx answers will not change on retry; network trouble and 5xx may. */
export function shouldRetry(failureCount: number, error: unknown): boolean {
  if (error instanceof ApiError && error.status >= 400 && error.status < 500) return false;
  if (error instanceof NetworkError) return failureCount < 1;
  return failureCount < 2;
}

export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 30_000,
        gcTime: 5 * 60_000,
        retry: shouldRetry,
        refetchOnWindowFocus: false,
        // Try the request even when the browser reports "offline": the service worker may
        // still answer it from the last good response (network-first API cache).
        networkMode: 'offlineFirst',
      },
      mutations: { networkMode: 'offlineFirst' },
    },
  });
}

export const queryClient = createQueryClient();
