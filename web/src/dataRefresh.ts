import type { QueryClient } from '@tanstack/react-query';
import { NATIVE_DATA_EVENT } from './native';

/** Burst of events (a sync uploads many files) becomes one refetch this long after the last one. */
export const DATA_REFRESH_DEBOUNCE_MS = 2_000;

/**
 * Keeps what the page shows from going stale while the page stays open (the Android app keeps one
 * page open for days):
 *
 * - the app dispatches `nimrag:data` on `window` after a sync's files reached the server; every
 *   query is invalidated (the active ones refetch at once), debounced so a burst is one refetch;
 * - when the page becomes visible again, the active queries that went stale meanwhile refetch
 *   (`refetchOnWindowFocus` is off on purpose, so this is the one place it happens).
 *
 * Returns the function that removes the listeners.
 */
export function listenForFreshData(
  client: QueryClient,
  debounceMs: number = DATA_REFRESH_DEBOUNCE_MS,
  win: Window = window,
): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;

  const onData = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      void client.invalidateQueries();
    }, debounceMs);
  };

  const onVisibility = () => {
    if (win.document.visibilityState === 'visible') void client.refetchQueries({ type: 'active', stale: true });
  };

  win.addEventListener(NATIVE_DATA_EVENT, onData);
  win.document.addEventListener('visibilitychange', onVisibility);
  return () => {
    if (timer !== undefined) clearTimeout(timer);
    win.removeEventListener(NATIVE_DATA_EVENT, onData);
    win.document.removeEventListener('visibilitychange', onVisibility);
  };
}
