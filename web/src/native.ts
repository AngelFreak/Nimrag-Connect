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
  /** Starts a sync with the watch. May return a promise that settles when the sync ends. */
  syncNow?: () => void | Promise<void>;
  status?: () => NativeStatus;
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
