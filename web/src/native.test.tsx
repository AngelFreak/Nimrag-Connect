import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { AppShell, WatchLink } from './components';
import { useNativeRouting } from './lib/hooks';
import { NATIVE_NAVIGATE_EVENT, hasNativeSession, hasNativeWatch, isNativeApp, navigatePath, nativeDownload, notifySessionExpired, openWatch, reportRoute } from './native';

afterEach(() => {
  delete window.NimragApp;
});

function renderAt(ui: React.ReactNode, path = '/') {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={[path]}>{ui}</MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('bridge helpers', () => {
  it('know nothing without the app', () => {
    expect(isNativeApp()).toBe(false);
    expect(hasNativeWatch()).toBe(false);
    expect(openWatch()).toBe(false);
    expect(() => reportRoute('/health')).not.toThrow();
  });

  it('nativeDownload asks the app only when it has the call', () => {
    expect(nativeDownload('/api/export', 'x.zip')).toBe(false);
    window.NimragApp = { native: true };
    expect(nativeDownload('/api/export', 'x.zip')).toBe(false);
    const download = vi.fn();
    window.NimragApp = { native: true, download };
    expect(nativeDownload('/api/export', 'x.zip')).toBe(true);
    expect(download).toHaveBeenCalledWith('/api/export', 'x.zip');
    window.NimragApp = { native: true, download: () => { throw new Error('gone'); } };
    expect(nativeDownload('/api/export', null)).toBe(false);
  });

  it('isNativeApp needs the native flag', () => {
    window.NimragApp = { syncNow: () => {} };
    expect(isNativeApp()).toBe(false);
    window.NimragApp = { native: true };
    expect(isNativeApp()).toBe(true);
  });

  it('openWatch calls the app and survives a failing bridge', () => {
    const open = vi.fn();
    window.NimragApp = { native: true, openWatch: open };
    expect(hasNativeWatch()).toBe(true);
    expect(openWatch()).toBe(true);
    expect(open).toHaveBeenCalledTimes(1);
    window.NimragApp = {
      native: true,
      openWatch: () => {
        throw new Error('bridge down');
      },
    };
    expect(openWatch()).toBe(false);
  });

  it('notifySessionExpired reaches only an app that can take the session over', () => {
    expect(hasNativeSession()).toBe(false);
    expect(notifySessionExpired()).toBe(false);
    window.NimragApp = { native: true };
    expect(notifySessionExpired()).toBe(false);
    const sessionExpired = vi.fn();
    window.NimragApp = { native: true, sessionExpired };
    expect(hasNativeSession()).toBe(true);
    expect(notifySessionExpired()).toBe(true);
    expect(sessionExpired).toHaveBeenCalledTimes(1);
    // Without the native flag the call is not trusted either.
    window.NimragApp = { sessionExpired };
    expect(notifySessionExpired()).toBe(false);
    expect(sessionExpired).toHaveBeenCalledTimes(1);
  });

  it('reportRoute passes the path and swallows errors', () => {
    const onRoute = vi.fn();
    window.NimragApp = { native: true, onRoute };
    reportRoute('/strength');
    expect(onRoute).toHaveBeenCalledWith('/strength');
    window.NimragApp = {
      native: true,
      onRoute: () => {
        throw new Error('bridge down');
      },
    };
    expect(() => reportRoute('/')).not.toThrow();
  });

  it('navigatePath accepts in-app paths only', () => {
    const ev = (detail: unknown) => new CustomEvent(NATIVE_NAVIGATE_EVENT, { detail });
    expect(navigatePath(ev({ path: '/health' }))).toBe('/health');
    expect(navigatePath(ev({ path: '/strength' }))).toBe('/strength');
    expect(navigatePath(ev({ path: 'https://evil.example/' }))).toBeNull();
    expect(navigatePath(ev({ path: '//evil.example' }))).toBeNull();
    expect(navigatePath(ev({ path: '/\\evil.example' }))).toBeNull();
    expect(navigatePath(ev({ path: 5 }))).toBeNull();
    expect(navigatePath(ev(null))).toBeNull();
    expect(navigatePath(new Event(NATIVE_NAVIGATE_EVENT))).toBeNull();
  });
});

function Probe() {
  useNativeRouting();
  const l = useLocation();
  return <p data-testid="where">{l.pathname + l.search}</p>;
}

describe('router and the app', () => {
  it('reports the first page and every route change with its query', async () => {
    const onRoute = vi.fn();
    window.NimragApp = { native: true, onRoute };
    renderAt(<Probe />, '/health');
    expect(onRoute).toHaveBeenLastCalledWith('/health');
    act(() => {
      window.dispatchEvent(new CustomEvent(NATIVE_NAVIGATE_EVENT, { detail: { path: '/strength' } }));
    });
    expect(screen.getByTestId('where')).toHaveTextContent('/strength');
    expect(onRoute).toHaveBeenLastCalledWith('/strength');
    expect(onRoute).toHaveBeenCalledTimes(2);
  });

  it('follows nimrag:navigate and ignores paths outside the app', () => {
    renderAt(<Probe />, '/');
    act(() => {
      window.dispatchEvent(new CustomEvent(NATIVE_NAVIGATE_EVENT, { detail: { path: '/sleep' } }));
    });
    expect(screen.getByTestId('where')).toHaveTextContent('/sleep');
    act(() => {
      window.dispatchEvent(new CustomEvent(NATIVE_NAVIGATE_EVENT, { detail: { path: 'https://evil.example/' } }));
    });
    expect(screen.getByTestId('where')).toHaveTextContent('/sleep');
  });

  it('stops listening when unmounted', () => {
    const view = renderAt(<Probe />, '/');
    view.unmount();
    expect(() => window.dispatchEvent(new CustomEvent(NATIVE_NAVIGATE_EVENT, { detail: { path: '/health' } }))).not.toThrow();
  });
});

describe('web shell inside the app', () => {
  it('hides the bottom bar when native, shows it in a browser', () => {
    const { unmount } = renderAt(<AppShell tabs>content</AppShell>);
    expect(document.querySelector('.bottom-nav')).not.toBeNull();
    unmount();
    window.NimragApp = { native: true };
    renderAt(<AppShell tabs>content</AppShell>);
    expect(document.querySelector('.bottom-nav')).toBeNull();
  });

  it('the watch icon calls openWatch() instead of linking to /devices', async () => {
    const open = vi.fn();
    window.NimragApp = { native: true, openWatch: open };
    renderAt(<WatchLink link="connected" />);
    expect(screen.queryByRole('link', { name: /Watch/ })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Watch connected' }));
    expect(open).toHaveBeenCalledTimes(1);
  });

  it('the watch icon still links to /devices in a browser', () => {
    renderAt(<WatchLink link="waiting" />);
    expect(screen.getByRole('link', { name: 'Watch waiting to sync' })).toHaveAttribute('href', '/devices');
  });
});
