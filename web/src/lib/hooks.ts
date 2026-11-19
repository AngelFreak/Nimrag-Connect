import { useCallback, useEffect, useState, useSyncExternalStore } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { NATIVE_NAVIGATE_EVENT, NATIVE_STATUS_EVENT, navigatePath, readNativeStatus, reportRoute, type NativeStatus } from '../native';

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

/**
 * Android shell: tells the app every page the web view shows (`onRoute`) and follows the app's
 * `nimrag:navigate` event when one of its tabs is tapped. Does nothing in a plain browser.
 */
export function useNativeRouting(): void {
  const { pathname, search } = useLocation();
  const navigate = useNavigate();
  useEffect(() => {
    reportRoute(pathname + search);
  }, [pathname, search]);
  useEffect(() => {
    const follow = (event: Event) => {
      const path = navigatePath(event);
      if (path !== null) void navigate(path);
    };
    window.addEventListener(NATIVE_NAVIGATE_EVENT, follow);
    return () => window.removeEventListener(NATIVE_NAVIGATE_EVENT, follow);
  }, [navigate]);
}
