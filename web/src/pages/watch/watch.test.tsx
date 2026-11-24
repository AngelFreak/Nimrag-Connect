import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../../App';
import type { DevicesResponse, SettingsResponse, StorageResponse } from '../../api/generated';
import { json, mockApi } from '../body/testUtils';
import { watchStatus } from '../WatchPage';
import { fmtAgo, isStale } from './format';
import { parseItems } from './WatchDetailsPage';

const NOW = Date.parse('2026-11-19T08:00:00+01:00');

const settings = { timezone: 'Europe/Copenhagen', step_goal: 8000, intensity_goal_min: 150, home_cards: { shown: [], hidden: [] }, sample: false } as unknown as SettingsResponse;
const me = { authenticated: true, username: 'demo', role: 'user', timezone: 'Europe/Copenhagen', sample: false };

const devices = (over: Partial<DevicesResponse> = {}): DevicesResponse => ({
  sample: false,
  watch: { model: null, firmware: null, battery_percent: 74, battery_at: '2026-11-19T07:42:00+01:00', last_data_at: '2026-11-19T07:40:00+01:00' },
  last_sync_at: '2026-11-19T07:42:00+01:00',
  new_files_24h: 5,
  upload_devices: [{ id: 1, name: 'Pixel 9a', created_at: '2026-11-02T10:00:00+01:00', last_used_at: '2026-11-19T07:42:00+01:00', active: true }],
  ...over,
});

const storage = (over: Partial<StorageResponse> = {}): StorageResponse => ({
  sample: false,
  files: 1284,
  bytes: 212 * 1024 * 1024,
  by_source: [{ source: 'watch', files: 1200, bytes: 200 * 1024 * 1024 }],
  parsed_ok: 1280,
  parsed_partial: 3,
  parsed_error: 1,
  database_bytes: 5 * 1024 * 1024,
  last_backup_at: null,
  ...over,
});

const tokens = { tokens: [{ id: 1, name: 'Pixel 9a', created_at: '2026-11-02T10:00:00+01:00', last_used_at: '2026-11-19T07:42:00+01:00', revoked_at: null, active: true, current: false }] };

function Where() {
  const l = useLocation();
  return <div data-testid="where">{l.pathname + l.search}</div>;
}

function renderApp(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <App />
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function routes(over: Record<string, unknown> = {}) {
  return { '/api/me': me, '/api/devices': devices(), '/api/storage': storage(), '/api/settings': settings, '/api/tokens': tokens, ...over };
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  delete window.NimragApp;
});

describe('Watch page', () => {
  it('is called Watch and shows the status card, then the Server, Sync and More groups', async () => {
    mockApi(routes());
    renderApp('/devices');
    expect(await screen.findByRole('heading', { level: 1, name: 'Watch' })).toBeInTheDocument();
    const card = await screen.findByRole('region', { name: 'Watch status' });
    expect(within(card).getByRole('heading', { name: 'Garmin watch' })).toBeInTheDocument();
    const fact = (label: string) => within(card).getByText(label, { selector: 'dt' }).nextElementSibling as HTMLElement;
    expect(fact('Battery')).toHaveTextContent('74%');
    expect(fact('New files, 24 h')).toHaveTextContent('5');
    expect(fact('Synced')).toHaveTextContent(/ago$/);
    expect(fact('Latest data')).toHaveTextContent(/ago$/);
    // One status chip.
    expect(card.querySelectorAll('.chip')).toHaveLength(1);

    const titles = Array.from(document.querySelectorAll('.sgroup__title')).map((h) => h.textContent);
    expect(titles).toEqual(['Server', 'Sync', 'More']);
    expect(screen.getByText('Address').closest('.srow')).toHaveTextContent(window.location.host);
    expect(await screen.findByText('Reachable')).toBeInTheDocument();
    expect(screen.getByText('5.0 MB')).toBeInTheDocument();
    expect(screen.getByText('1,284 files, 212 MB')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Phones Pixel 9a' })).toHaveAttribute('href', '/profile/phones');
    expect(await screen.findByRole('link', { name: 'Details 4 files not fully read' })).toHaveAttribute('href', '/devices/details');
  });

  it('has no duplicate phones list and no raw counters on the page', async () => {
    mockApi(routes());
    renderApp('/devices');
    await screen.findByRole('link', { name: /^Details/ });
    expect(screen.queryByRole('table', { name: 'Upload devices' })).not.toBeInTheDocument();
    expect(screen.queryByText('Read in part')).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Manage signed-in phones' })).not.toBeInTheDocument();
  });

  it('uses the real model name when the API has one', async () => {
    mockApi(routes({ '/api/devices': devices({ watch: { ...devices().watch, model: 'Instinct 2' } }) }));
    renderApp('/devices');
    expect(await screen.findByRole('heading', { name: 'Instinct 2' })).toBeInTheDocument();
  });

  it('says "unavailable" for what it should know and does not know', async () => {
    mockApi(routes({ '/api/devices': devices({ watch: { model: null, firmware: null, battery_percent: null, battery_at: null, last_data_at: null }, last_sync_at: null }) }));
    renderApp('/devices');
    const card = await screen.findByRole('region', { name: 'Watch status' });
    expect(within(card).getByText('Battery', { selector: 'dt' }).nextElementSibling).toHaveTextContent('unavailable');
    expect(within(card).getByText('Synced', { selector: 'dt' }).nextElementSibling).toHaveTextContent('Never');
    expect(within(card).getByText('No data yet')).toBeInTheDocument();
  });

  it('judges freshness by the newest watch data when nothing was ever uploaded (an import)', async () => {
    const recent = new Date(Date.now() - 3_600_000).toISOString();
    mockApi(routes({ '/api/devices': devices({ watch: { model: null, firmware: null, battery_percent: 44, battery_at: recent, last_data_at: recent }, last_sync_at: null }) }));
    renderApp('/devices');
    const card = await screen.findByRole('region', { name: 'Watch status' });
    expect(within(card).getByText('Up to date')).toBeInTheDocument();
  });

  it('shows Sync now only inside the app, where it asks the watch', async () => {
    mockApi(routes());
    const { unmount } = renderApp('/devices');
    await screen.findByRole('region', { name: 'Watch status' });
    expect(screen.queryByRole('button', { name: 'Sync now' })).not.toBeInTheDocument();
    unmount();
    const syncNow = vi.fn();
    window.NimragApp = { native: true, syncNow };
    mockApi(routes());
    renderApp('/devices');
    const card = await screen.findByRole('region', { name: 'Watch status' });
    await userEvent.click(within(card).getByRole('button', { name: 'Sync now' }));
    expect(syncNow).toHaveBeenCalledTimes(1);
  });

  it('in the app, a connection problem is one banner with one fix', async () => {
    const syncNow = vi.fn();
    window.NimragApp = { native: true, syncNow, status: () => ({ link: 'problem', lastSyncAt: null }) };
    mockApi(routes());
    renderApp('/devices');
    expect(await screen.findByText('The phone could not reach your watch.')).toBeInTheDocument();
    expect(screen.getByText('Connection problem', { selector: '.chip' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(syncNow).toHaveBeenCalled();
  });

  it('says so when a request fails and offers a retry', async () => {
    let fail = true;
    mockApi(routes({ '/api/devices': () => (fail ? json({ error: 'database busy' }, { status: 500 }) : json(devices())) }));
    renderApp('/devices');
    expect(await screen.findAllByRole('alert')).not.toHaveLength(0);
    expect(screen.getByText('database busy')).toBeInTheDocument();
    fail = false;
    await userEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByRole('heading', { name: 'Garmin watch' })).toBeInTheDocument();
  });

  it('a failed storage request leaves the rest of the page and offers a retry', async () => {
    mockApi(routes({ '/api/storage': () => json({ error: 'disk gone' }, { status: 500 }) }));
    renderApp('/devices');
    expect(await screen.findByText('Not answering')).toBeInTheDocument();
    expect(screen.getByText('Could not load the server details')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Garmin watch' })).toBeInTheDocument();
  });
});

describe('Watch details', () => {
  it('shows the counters that are not zero, the sentence, and the sources', async () => {
    mockApi(routes());
    renderApp('/devices/details');
    expect(await screen.findByRole('heading', { level: 1, name: 'Details' })).toBeInTheDocument();
    expect(await screen.findByText('Read in part')).toBeInTheDocument();
    expect(screen.getByText('Read in part').nextElementSibling).toHaveTextContent('3');
    expect(screen.getByText('Could not be read').nextElementSibling).toHaveTextContent('1');
    expect(screen.getByText('Read without trouble').nextElementSibling).toHaveTextContent('1,280');
    expect(screen.getByText(/Original files are kept as the watch sent them/)).toBeInTheDocument();
    expect(screen.getByRole('table', { name: 'Original files by source' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to watch' })).toHaveAttribute('href', '/devices');
  });

  it('shows no counter at all while every one is zero', async () => {
    mockApi(routes({ '/api/storage': storage({ parsed_ok: 0, parsed_partial: 0, parsed_error: 0, files: 0, by_source: [] }) }));
    renderApp('/devices/details');
    expect(await screen.findByText('No files have been read yet.')).toBeInTheDocument();
    for (const label of ['Read without trouble', 'Read in part', 'Could not be read']) expect(screen.queryByText(label)).not.toBeInTheDocument();
  });

  it('parseItems leaves out the zero counters and marks the problems', () => {
    expect(parseItems({ parsed_ok: 0, parsed_partial: 0, parsed_error: 0 })).toBeNull();
    expect(parseItems({ parsed_ok: 10, parsed_partial: 0, parsed_error: 2 })).toEqual([
      { label: 'Read without trouble', value: '10' },
      { label: 'Could not be read', value: '2', tone: 'warn' },
    ]);
  });
});

describe('watch helpers', () => {
  it('fmtAgo reads as words', () => {
    const at = (ms: number) => new Date(NOW - ms).toISOString();
    expect(fmtAgo(at(20_000), NOW)).toBe('just now');
    expect(fmtAgo(at(5 * 60_000), NOW)).toBe('5 min ago');
    expect(fmtAgo(at(2 * 3_600_000 + 5), NOW)).toBe('2 h ago');
    expect(fmtAgo(at(24 * 3_600_000), NOW)).toBe('1 day ago');
    expect(fmtAgo(at(3 * 86_400_000), NOW)).toBe('3 days ago');
    expect(fmtAgo(at(21 * 86_400_000), NOW)).toBe('3 weeks ago');
    expect(fmtAgo(at(90 * 86_400_000), NOW)).toBe('3 months ago');
    expect(fmtAgo(new Date(NOW + 5_000).toISOString(), NOW)).toBe('just now');
    expect(fmtAgo(null, NOW)).toBeNull();
    expect(fmtAgo('not a time', NOW)).toBeNull();
  });

  it('isStale is true for no sync or a sync over a day old', () => {
    expect(isStale(null, NOW)).toBe(true);
    expect(isStale(new Date(NOW - 2 * 3_600_000).toISOString(), NOW)).toBe(false);
    expect(isStale(new Date(NOW - 30 * 3_600_000).toISOString(), NOW)).toBe(true);
  });

  it('watchStatus prefers the phone link and falls back to how fresh the data is', () => {
    const fresh = new Date(NOW - 3_600_000).toISOString();
    expect(watchStatus('connected', null, NOW)).toBe('Connected');
    expect(watchStatus('waiting', fresh, NOW)).toBe('Waiting to sync');
    expect(watchStatus('problem', fresh, NOW)).toBe('Connection problem');
    expect(watchStatus('unknown', null, NOW)).toBe('No data yet');
    expect(watchStatus('unknown', fresh, NOW)).toBe('Up to date');
    expect(watchStatus('unknown', new Date(NOW - 3 * 86_400_000).toISOString(), NOW)).toBe('Not synced lately');
  });
});
