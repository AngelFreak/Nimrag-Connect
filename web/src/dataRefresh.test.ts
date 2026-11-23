import { QueryClient, QueryObserver } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DATA_REFRESH_DEBOUNCE_MS, listenForFreshData, refetchWhenWorkerTakesOver } from './dataRefresh';
import { NATIVE_DATA_EVENT } from './native';

function setVisibility(state: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state });
  document.dispatchEvent(new Event('visibilitychange'));
}

describe('listenForFreshData', () => {
  let client: QueryClient;
  let stop: () => void;
  let invalidate: ReturnType<typeof vi.spyOn>;
  let refetch: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.useFakeTimers();
    client = new QueryClient();
    invalidate = vi.spyOn(client, 'invalidateQueries').mockResolvedValue();
    refetch = vi.spyOn(client, 'refetchQueries').mockResolvedValue();
    stop = listenForFreshData(client);
  });

  afterEach(() => {
    stop();
    setVisibility('visible');
    vi.useRealTimers();
  });

  it('uses the event name the app dispatches', () => {
    expect(NATIVE_DATA_EVENT).toBe('nimrag:data');
  });

  it('invalidates every query after the debounce', () => {
    window.dispatchEvent(new CustomEvent(NATIVE_DATA_EVENT));
    expect(invalidate).not.toHaveBeenCalled();
    vi.advanceTimersByTime(DATA_REFRESH_DEBOUNCE_MS - 1);
    expect(invalidate).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith();
  });

  it('turns a burst of events into one invalidation, counted from the last event', () => {
    window.dispatchEvent(new CustomEvent(NATIVE_DATA_EVENT));
    vi.advanceTimersByTime(1_500);
    window.dispatchEvent(new CustomEvent(NATIVE_DATA_EVENT));
    vi.advanceTimersByTime(1_500);
    window.dispatchEvent(new CustomEvent(NATIVE_DATA_EVENT));
    vi.advanceTimersByTime(DATA_REFRESH_DEBOUNCE_MS - 1);
    expect(invalidate).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(invalidate).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(60_000);
    expect(invalidate).toHaveBeenCalledTimes(1);
  });

  it('a later event after the burst invalidates again', () => {
    window.dispatchEvent(new CustomEvent(NATIVE_DATA_EVENT));
    vi.advanceTimersByTime(DATA_REFRESH_DEBOUNCE_MS);
    window.dispatchEvent(new CustomEvent(NATIVE_DATA_EVENT));
    vi.advanceTimersByTime(DATA_REFRESH_DEBOUNCE_MS);
    expect(invalidate).toHaveBeenCalledTimes(2);
  });

  it('refetches stale active queries when the page becomes visible, not when it is hidden', () => {
    setVisibility('hidden');
    expect(refetch).not.toHaveBeenCalled();
    setVisibility('visible');
    expect(refetch).toHaveBeenCalledTimes(1);
    expect(refetch).toHaveBeenCalledWith({ type: 'active', stale: true });
  });

  it('stops listening and drops a pending refetch when removed', () => {
    window.dispatchEvent(new CustomEvent(NATIVE_DATA_EVENT));
    stop();
    vi.advanceTimersByTime(DATA_REFRESH_DEBOUNCE_MS * 2);
    window.dispatchEvent(new CustomEvent(NATIVE_DATA_EVENT));
    setVisibility('visible');
    vi.advanceTimersByTime(DATA_REFRESH_DEBOUNCE_MS * 2);
    expect(invalidate).not.toHaveBeenCalled();
    expect(refetch).not.toHaveBeenCalled();
  });
});

describe('listenForFreshData with a real query client', () => {
  afterEach(() => {
    setVisibility('visible');
    vi.useRealTimers();
  });

  it('on visibility only active queries older than staleTime refetch; an event refetches active ones', async () => {
    vi.useFakeTimers();
    const client = new QueryClient({ defaultOptions: { queries: { staleTime: 30_000 } } });
    const active = vi.fn().mockResolvedValue('a');
    const inactive = vi.fn().mockResolvedValue('b');
    const unsubscribe = new QueryObserver(client, { queryKey: ['active'], queryFn: active }).subscribe(() => {});
    await client.fetchQuery({ queryKey: ['inactive'], queryFn: inactive });
    await vi.advanceTimersByTimeAsync(0);
    expect(active).toHaveBeenCalledTimes(1);
    const stop = listenForFreshData(client);

    // Still fresh: nothing to fetch.
    setVisibility('visible');
    await vi.advanceTimersByTimeAsync(0);
    expect(active).toHaveBeenCalledTimes(1);

    // Stale now: the active query refetches, the one nobody shows does not.
    await vi.advanceTimersByTimeAsync(31_000);
    setVisibility('visible');
    await vi.advanceTimersByTimeAsync(0);
    expect(active).toHaveBeenCalledTimes(2);
    expect(inactive).toHaveBeenCalledTimes(1);

    // New data: fresh queries refetch too, after the debounce.
    window.dispatchEvent(new CustomEvent(NATIVE_DATA_EVENT));
    await vi.advanceTimersByTimeAsync(DATA_REFRESH_DEBOUNCE_MS);
    expect(active).toHaveBeenCalledTimes(3);
    expect(inactive).toHaveBeenCalledTimes(1);

    stop();
    unsubscribe();
    client.clear();
  });
});

describe('refetchWhenWorkerTakesOver', () => {
  it('refetches the active queries when the service worker takes over the page', () => {
    const client = new QueryClient();
    const refetch = vi.spyOn(client, 'refetchQueries').mockResolvedValue();
    const container = new EventTarget() as ServiceWorkerContainer;
    const stop = refetchWhenWorkerTakesOver(client, container);

    expect(refetch).not.toHaveBeenCalled();
    container.dispatchEvent(new Event('controllerchange'));
    expect(refetch).toHaveBeenCalledWith({ type: 'active' });

    stop();
    container.dispatchEvent(new Event('controllerchange'));
    expect(refetch).toHaveBeenCalledTimes(1);
  });

  it('does nothing without service workers', () => {
    const client = new QueryClient();
    const stop = refetchWhenWorkerTakesOver(client, undefined);
    expect(() => stop()).not.toThrow();
  });
});
