import { act, renderHook } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NATIVE_STATUS_EVENT } from '../native';
import { useSync } from './SyncControls';

// The app reports `syncing` in its status and fires the status event when it changes.
let syncing = false;
function setSyncing(next: boolean) {
  syncing = next;
  window.dispatchEvent(new Event(NATIVE_STATUS_EVENT));
}

function renderSync() {
  const client = new QueryClient();
  return renderHook(() => useSync(), {
    wrapper: ({ children }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>,
  });
}

const wait = (ms: number) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));

describe('useSync in the app', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    syncing = false;
    window.NimragApp = { native: true, syncNow: vi.fn(), status: () => ({ link: 'connected', lastSyncAt: null, syncing }) };
  });
  afterEach(() => {
    vi.useRealTimers();
    delete window.NimragApp;
  });

  it('spins after a tap until the pull has started and ended', async () => {
    const { result } = renderSync();
    act(() => result.current.sync());
    expect(result.current.spinning).toBe(true);
    // syncNow returned at once, but the pull has not started yet.
    await wait(2_000);
    expect(result.current.spinning).toBe(true);
    act(() => setSyncing(true));
    await wait(30_000);
    expect(result.current.spinning).toBe(true);
    act(() => setSyncing(false));
    expect(result.current.spinning).toBe(false);
  });

  it('stops when the pull never starts', async () => {
    const { result } = renderSync();
    act(() => result.current.sync());
    await wait(14_000);
    expect(result.current.spinning).toBe(true);
    await wait(2_000);
    expect(result.current.spinning).toBe(false);
  });

  it('spins for a pull the app started on its own', () => {
    const { result } = renderSync();
    expect(result.current.spinning).toBe(false);
    act(() => setSyncing(true));
    expect(result.current.spinning).toBe(true);
    act(() => setSyncing(false));
    expect(result.current.spinning).toBe(false);
  });

  it('a tap during a running pull spins until that pull ends', async () => {
    const { result } = renderSync();
    act(() => setSyncing(true));
    act(() => result.current.sync());
    await wait(1_000);
    act(() => setSyncing(false));
    expect(result.current.spinning).toBe(false);
  });
});
