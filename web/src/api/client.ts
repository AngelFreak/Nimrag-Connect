// Small fetch wrapper for the JSON API. It knows nothing about individual endpoints; the
// request and response types come from web/src/api/generated/ (written by the server).
//
// - same-origin cookies (the session cookie is HttpOnly, the page never sees it)
// - `X-Nimrag: 1` on every request that changes something (CSRF guard next to SameSite)
// - typed errors, so callers can tell "wrong password" from "server down"
// - 401 on a session-protected call clears the offline API cache and sends the user to /login;
//   inside the Android app the app signs in again natively instead (see native.ts)

import { notifySessionExpired } from '../native';

export class ApiError extends Error {
  readonly status: number;
  readonly body: unknown;
  constructor(status: number, message: string, body?: unknown) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.body = body;
  }
}

/** 401: not logged in, or the session expired. */
export class UnauthorizedError extends ApiError {
  constructor(message = 'Not logged in', body?: unknown) {
    super(401, message, body);
    this.name = 'UnauthorizedError';
  }
}

/** The request never produced a response: offline, server down, DNS, TLS. */
export class NetworkError extends Error {
  constructor(message = 'Could not reach the server', options?: { cause?: unknown }) {
    super(message, options);
    this.name = 'NetworkError';
  }
}

export interface RequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
  /** Serialised as JSON when present. */
  body?: unknown;
  signal?: AbortSignal;
  /** Set false for calls where 401 is an answer (the login form), not a lost session. */
  redirectOn401?: boolean;
}

const SAFE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS']);

type UnauthorizedHandler = () => void;

function defaultUnauthorized(): void {
  if (window.location.pathname.startsWith('/login') || window.location.pathname.startsWith('/setup')) return;
  const next = window.location.pathname + window.location.search;
  window.location.assign(`/login?next=${encodeURIComponent(next)}`);
}

let onUnauthorized: UnauthorizedHandler = defaultUnauthorized;

/** The router installs a handler that navigates without a full page load. */
export function setUnauthorizedHandler(handler: UnauthorizedHandler | null): void {
  onUnauthorized = handler ?? defaultUnauthorized;
}

async function readBody(res: Response): Promise<unknown> {
  if (res.status === 204) return undefined;
  const text = await res.text();
  if (!text) return undefined;
  const type = res.headers.get('content-type') ?? '';
  if (type.includes('json')) {
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  }
  return text;
}

function errorMessage(body: unknown, fallback: string): string {
  if (body && typeof body === 'object' && 'error' in body && typeof body.error === 'string') return body.error;
  if (typeof body === 'string' && body.length > 0 && body.length < 200 && !body.trimStart().startsWith('<')) return body;
  return fallback;
}

export async function apiFetch<T = unknown>(path: string, options: RequestOptions = {}): Promise<T> {
  const method = options.method ?? 'GET';
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (!SAFE_METHODS.has(method)) headers['X-Nimrag'] = '1';
  let body: string | undefined;
  if (options.body !== undefined) {
    headers['Content-Type'] = 'application/json';
    body = JSON.stringify(options.body);
  }

  let res: Response;
  try {
    res = await fetch(path, { method, headers, body, credentials: 'same-origin', signal: options.signal ?? null });
  } catch (cause) {
    if (cause instanceof DOMException && cause.name === 'AbortError') throw cause;
    throw new NetworkError(undefined, { cause });
  }

  const data = await readBody(res);
  if (res.ok) return data as T;

  if (res.status === 401) {
    if (options.redirectOn401 !== false) {
      // The session is gone: do not leave the last signed-in user's data readable offline.
      await clearOfflineApiCache();
      // In the Android app the app itself signs in again (fingerprint or password); the web app
      // shows no login page there. A browser, or an app without the call, gets the login page.
      if (!notifySessionExpired()) onUnauthorized();
    }
    throw new UnauthorizedError(errorMessage(data, 'Not logged in'), data);
  }
  throw new ApiError(res.status, errorMessage(data, res.statusText || `HTTP ${res.status}`), data);
}

export const api = {
  get: <T = unknown>(path: string, options?: Pick<RequestOptions, 'signal'>) => apiFetch<T>(path, { ...options }),
  post: <T = unknown>(path: string, body?: unknown, options?: Omit<RequestOptions, 'method' | 'body'>) =>
    apiFetch<T>(path, { ...options, method: 'POST', body }),
  put: <T = unknown>(path: string, body?: unknown) => apiFetch<T>(path, { method: 'PUT', body }),
  patch: <T = unknown>(path: string, body?: unknown) => apiFetch<T>(path, { method: 'PATCH', body }),
  delete: <T = unknown>(path: string) => apiFetch<T>(path, { method: 'DELETE' }),
};

/** Name of the service-worker cache that holds API responses for offline viewing. */
export const API_CACHE = 'nimrag-api';

/** Drops cached API responses so the next person on this browser cannot read them offline. */
export async function clearOfflineApiCache(): Promise<void> {
  if (typeof caches === 'undefined') return;
  try {
    await caches.delete(API_CACHE);
  } catch {
    // Cache storage can be blocked; nothing to clear then.
  }
}
