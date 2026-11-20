// Optional bridge to the Android app's embedded web view (stage 3n). The app injects
// `window.NimragApp` before the page loads; in a normal browser it does not exist, so
// everything here is feature-detected and the web app works without it.

/** State of the phone-to-watch link, as the web UI shows it on the watch icon. */
export type LinkState = 'connected' | 'waiting' | 'problem' | 'unknown';

export interface NativeStatus {
  link: LinkState;
  /** ISO 8601 timestamp of the last completed sync, or null when there has not been one. */
  lastSyncAt: string | null;
}

export interface NimragAppBridge {
  /** True inside the Android app's web view (the app defines it; a plain browser has no bridge). */
  native?: boolean;
  /** Starts a sync with the watch. May return a promise that settles when the sync ends. */
  syncNow?: () => void | Promise<void>;
  status?: () => NativeStatus;
  /** Opens the app's own Watch screen (Devices and sync stays native in the app). */
  openWatch?: () => void;
  /** Tells the app which page the web view shows, so its bottom bar highlights the right tab. */
  onRoute?: (path: string) => void;
  /**
   * The server answered 401: the session is gone. The app signs in again natively (fingerprint or
   * password) and reloads the page; the web app shows no login page of its own meanwhile.
   */
  sessionExpired?: () => void;
}

declare global {
  interface Window {
    NimragApp?: NimragAppBridge;
  }
}

const LINK_STATES: readonly LinkState[] = ['connected', 'waiting', 'problem', 'unknown'];

export function getBridge(): NimragAppBridge | undefined {
  return typeof window === 'undefined' ? undefined : window.NimragApp;
}

export function hasNativeSync(): boolean {
  return typeof getBridge()?.syncNow === 'function';
}

/** True inside the Android app: the web bottom bar is hidden then (the app has its own). */
export function isNativeApp(): boolean {
  return getBridge()?.native === true;
}

/** True when the app can open its own Watch screen. */
export function hasNativeWatch(): boolean {
  return typeof getBridge()?.openWatch === 'function';
}

/** Opens the app's Watch screen. Returns false when there is no app to ask (the caller then links to /devices). */
export function openWatch(): boolean {
  const bridge = getBridge();
  if (typeof bridge?.openWatch !== 'function') return false;
  try {
    bridge.openWatch();
    return true;
  } catch {
    return false;
  }
}

/** True inside the app when it can take over a lost session (older app versions cannot). */
export function hasNativeSession(): boolean {
  return isNativeApp() && typeof getBridge()?.sessionExpired === 'function';
}

/**
 * Tells the app the session is gone. Returns true when the app took it over (the web app must
 * then not show its own login page); false in a plain browser or an app without the call.
 */
export function notifySessionExpired(): boolean {
  const bridge = getBridge();
  if (!hasNativeSession() || typeof bridge?.sessionExpired !== 'function') return false;
  try {
    bridge.sessionExpired();
    return true;
  } catch {
    return false;
  }
}

/** Reports the current page (path and query) to the app. Never throws: the app is optional. */
export function reportRoute(path: string): void {
  const bridge = getBridge();
  if (typeof bridge?.onRoute !== 'function') return;
  try {
    bridge.onRoute(path);
  } catch {
    // A failing bridge must not break navigation.
  }
}

/**
 * The path of a `nimrag:navigate` event, or null when it is not an in-app path. Only paths of
 * this app are followed ("/strength"); anything else is ignored.
 */
export function navigatePath(event: Event): string | null {
  const detail: unknown = event instanceof CustomEvent ? event.detail : null;
  const path = detail && typeof detail === 'object' && 'path' in detail ? (detail as { path: unknown }).path : null;
  if (typeof path !== 'string' || !path.startsWith('/') || path.startsWith('//') || path.includes('\\')) return null;
  return path;
}

/** Reads the native status defensively; anything malformed becomes "unknown". */
export function readNativeStatus(): NativeStatus {
  try {
    const raw = getBridge()?.status?.();
    if (!raw) return { link: 'unknown', lastSyncAt: null };
    const link = LINK_STATES.includes(raw.link) ? raw.link : 'unknown';
    const lastSyncAt = typeof raw.lastSyncAt === 'string' ? raw.lastSyncAt : null;
    return { link, lastSyncAt };
  } catch {
    return { link: 'unknown', lastSyncAt: null };
  }
}

/** Event the app dispatches on `window` when the link state or last sync time changes. */
export const NATIVE_STATUS_EVENT = 'nimrag:status';

/** Event the app dispatches on `window` when one of its own tabs is tapped; `detail.path` is the page to show. */
export const NATIVE_NAVIGATE_EVENT = 'nimrag:navigate';

/**
 * Event the app dispatches on `window` when new watch data reached the server (after a sync's files
 * were uploaded); `src/dataRefresh.ts` refetches what the page shows.
 */
export const NATIVE_DATA_EVENT = 'nimrag:data';
