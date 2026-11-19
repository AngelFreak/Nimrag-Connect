import { useCallback, useEffect, useState, useSyncExternalStore } from 'react';
import { NATIVE_STATUS_EVENT, readNativeStatus, type NativeStatus } from '../native';

export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (onChange: () => void) => {
      if (typeof window === 'undefined' || !window.matchMedia) return () => {};
      const mql = window.matchMedia(query);
      mql.addEventListener('change', onChange);
      return () => mql.removeEventListener('change', onChange);
    },
    [query],
  );
  const get = useCallback(() => (typeof window !== 'undefined' && window.matchMedia ? window.matchMedia(query).matches : false), [query]);
  return useSyncExternalStore(subscribe, get, () => false);
}

/** True at the desktop layout breakpoint (the web dashboard of the design canvas). */
export function useIsDesktop(): boolean {
  return useMediaQuery('(min-width: 1024px)');
}

/** Sets the browser tab title ("Sleep · Nimrag Connect"). */
export function usePageTitle(title: string): void {
  useEffect(() => {
    document.title = title ? `${title} · Nimrag Connect` : 'Nimrag Connect';
  }, [title]);
}

/** Native link status, refreshed on the app's event and every 15 s while visible. */
export function useNativeStatus(): NativeStatus {
  const [status, setStatus] = useState<NativeStatus>(() => readNativeStatus());
  useEffect(() => {
    const refresh = () => setStatus(readNativeStatus());
    window.addEventListener(NATIVE_STATUS_EVENT, refresh);
    const id = window.setInterval(() => {
      if (document.visibilityState === 'visible') refresh();
    }, 15_000);
    return () => {
      window.removeEventListener(NATIVE_STATUS_EVENT, refresh);
      window.clearInterval(id);
    };
  }, []);
  return status;
}
