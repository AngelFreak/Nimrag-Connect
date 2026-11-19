import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError, NetworkError, UnauthorizedError, api, apiFetch, setUnauthorizedHandler } from './client';
import { shouldRetry } from './queryClient';

function jsonResponse(body: unknown, init: ResponseInit = {}): Response {
  return new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' }, ...init });
}

describe('api client', () => {
  const fetchMock = vi.fn<typeof fetch>();
  beforeEach(() => {
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => {
    fetchMock.mockReset();
    vi.unstubAllGlobals();
    setUnauthorizedHandler(null);
  });

  it('sends same-origin credentials and no CSRF header on GET', async () => {
    fetchMock.mockResolvedValue(jsonResponse({ ok: true }));
    await expect(api.get('/api/me')).resolves.toEqual({ ok: true });
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/me');
    expect(init.credentials).toBe('same-origin');
    expect((init.headers as Record<string, string>)['X-Nimrag']).toBeUndefined();
  });

  it('sends X-Nimrag: 1 and a JSON body on mutations', async () => {
    fetchMock.mockImplementation(() => Promise.resolve(jsonResponse({})));
    await api.post('/api/login', { password: 'x' });
    await api.patch('/api/settings', { timezone: 'Europe/Copenhagen' });
    await api.delete('/api/tokens/1');
    for (const call of fetchMock.mock.calls) {
      const init = call[1] as RequestInit;
      expect((init.headers as Record<string, string>)['X-Nimrag']).toBe('1');
    }
    const first = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(first.method).toBe('POST');
    expect(first.body).toBe('{"password":"x"}');
  });

  it('401 calls the unauthorized handler and throws UnauthorizedError', async () => {
    const handler = vi.fn();
    setUnauthorizedHandler(handler);
    fetchMock.mockResolvedValue(jsonResponse({ error: 'nope' }, { status: 401 }));
    await expect(api.get('/api/today')).rejects.toBeInstanceOf(UnauthorizedError);
    expect(handler).toHaveBeenCalledOnce();
  });

  it('401 on the login call is an answer, not a lost session', async () => {
    const handler = vi.fn();
    setUnauthorizedHandler(handler);
    fetchMock.mockResolvedValue(jsonResponse({ error: 'wrong password' }, { status: 401 }));
    await expect(api.post('/api/login', { password: 'bad' }, { redirectOn401: false })).rejects.toMatchObject({ status: 401, message: 'wrong password' });
    expect(handler).not.toHaveBeenCalled();
  });

  it('other HTTP errors become ApiError with status and body', async () => {
    fetchMock.mockResolvedValue(jsonResponse({ error: 'boom' }, { status: 500 }));
    const err = await apiFetch('/api/today').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err).toMatchObject({ status: 500, message: 'boom', body: { error: 'boom' } });
  });

  it('network failures become NetworkError', async () => {
    fetchMock.mockRejectedValue(new TypeError('Failed to fetch'));
    await expect(api.get('/api/today')).rejects.toBeInstanceOf(NetworkError);
  });

  it('does not retry 4xx answers', () => {
    expect(shouldRetry(0, new ApiError(404, 'missing'))).toBe(false);
    expect(shouldRetry(0, new ApiError(500, 'boom'))).toBe(true);
    expect(shouldRetry(2, new ApiError(500, 'boom'))).toBe(false);
    expect(shouldRetry(0, new NetworkError())).toBe(true);
  });
});
