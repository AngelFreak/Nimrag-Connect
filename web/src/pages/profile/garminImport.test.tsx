import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../../App';
import { ApiError, NetworkError, setUnauthorizedHandler, uploadFile } from '../../api/client';
import type { GarminImportResponse, SettingsResponse } from '../../api/generated';
import { mockApi } from '../body/testUtils';
import { importErrorMessage, isImportable, summarizeResult } from './garminImport';

/** Just enough of XMLHttpRequest for `uploadFile`, driven by the test. */
class FakeXhr {
  static all: FakeXhr[] = [];
  method = '';
  url = '';
  headers: Record<string, string> = {};
  body: unknown;
  withCredentials = false;
  aborted = false;
  status = 0;
  statusText = '';
  responseText = '';
  responseHeaders: Record<string, string> = {};
  upload: { onprogress: ((e: { lengthComputable: boolean; loaded: number; total: number }) => void) | null; onload: (() => void) | null } = {
    onprogress: null,
    onload: null,
  };
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onabort: (() => void) | null = null;

  open(method: string, url: string) {
    this.method = method;
    this.url = url;
  }
  setRequestHeader(name: string, value: string) {
    this.headers[name] = value;
  }
  getResponseHeader(name: string) {
    return this.responseHeaders[name.toLowerCase()] ?? null;
  }
  send(body: unknown) {
    this.body = body;
    FakeXhr.all.push(this);
  }
  abort() {
    this.aborted = true;
    this.onabort?.();
  }

  progress(loaded: number, total: number) {
    act(() => this.upload.onprogress?.({ lengthComputable: true, loaded, total }));
  }
  sent() {
    act(() => this.upload.onload?.());
  }
  respond(status: number, body: unknown) {
    this.status = status;
    this.responseText = JSON.stringify(body);
    this.responseHeaders['content-type'] = 'application/json';
    act(() => this.onload?.());
  }
}

const settings: SettingsResponse = {
  timezone: 'Europe/Copenhagen',
  step_goal: 8000,
  intensity_goal_min: 150, sleep_need_min: 480,
  home_cards: { shown: [], hidden: [] } as unknown as SettingsResponse['home_cards'],
  sample: false,
};
const me = { authenticated: true, username: 'demo', role: 'user', timezone: 'Europe/Copenhagen', sample: false };

const result = (over: Partial<GarminImportResponse> = {}): GarminImportResponse => ({
  name: 'export.zip',
  fit_found: 12,
  new: 9,
  already_present: 3,
  skipped: 40,
  failed: 0,
  errors: [],
  kinds: [
    { kind: 'activity', count: 10 },
    { kind: 'sleep', count: 2 },
  ],
  ...over,
});

function renderPage(path = '/profile/import-garmin') {
  FakeXhr.all = [];
  vi.stubGlobal('XMLHttpRequest', FakeXhr);
  mockApi({ '/api/me': me, '/api/settings': settings, '/api/tokens': { tokens: [] }, '/api/garmin': { state: 'off' }, '/api/about': { version: '0.1.0' } });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const invalidate = vi.spyOn(client, 'invalidateQueries');
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return { invalidate };
}

const file = (name: string, size = 100) => new File([new Uint8Array(size)], name);
const input = () => screen.findByLabelText('Garmin export files') as Promise<HTMLInputElement>;

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  setUnauthorizedHandler(null);
});

describe('Import from Garmin: the words', () => {
  it('takes zips and plain FIT files, whatever the case', () => {
    for (const ok of ['1234.zip', 'ride.FIT', 'Export (1).ZIP']) expect(isImportable(ok)).toBe(true);
    for (const no of ['notes.txt', 'a.zip.txt', 'zip', 'a.gpx']) expect(isImportable(no)).toBe(false);
  });

  it('says what was added, what was skipped and what failed', () => {
    const s = summarizeResult(result());
    expect(s.headline).toBe('9 new files, 3 already here.');
    expect(s.kinds).toBe('Activities 10 · Sleep 2');
    expect(s.notes).toEqual(['40 files were skipped because they are not FIT.']);
    expect(s.warn).toBe(false);

    expect(summarizeResult(result({ new: 1, already_present: 0, fit_found: 1, skipped: 1, kinds: [] })).headline).toBe('1 new file.');
    expect(summarizeResult(result({ new: 0, already_present: 12 })).headline).toBe('Nothing new: 12 files are already here.');

    const failed = summarizeResult(result({ failed: 2 }));
    expect(failed.warn).toBe(true);
    expect(failed.notes).toContain('2 files could not be read.');

    const none = summarizeResult(result({ fit_found: 0, new: 0, already_present: 0, skipped: 1, kinds: [] }));
    expect(none).toMatchObject({ headline: 'No FIT files found.', warn: true, kinds: null });
  });

  it('turns the server\'s refusals into plain sentences', () => {
    expect(importErrorMessage(new ApiError(413, 'file too large'))).toBe('This file is too big for the server.');
    expect(importErrorMessage(new ApiError(429, 'busy'))).toMatch(/busy with other imports/);
    expect(importErrorMessage(new ApiError(500, 'internal error'))).toBe('internal error');
    expect(importErrorMessage(new NetworkError())).toMatch(/Lost the connection/);
    expect(importErrorMessage(new Error('x'))).toMatch(/Something went wrong/);
  });
});

describe('Import from Garmin: the page', () => {
  it('is linked from the profile list, next to "Download my data"', async () => {
    renderPage('/profile');
    const link = await screen.findByRole('link', { name: /^Import from Garmin/ });
    expect(link).toHaveAttribute('href', '/profile/import-garmin');
  });

  it('explains the two exports and offers a normal file input', async () => {
    renderPage();
    expect(await screen.findByRole('heading', { level: 1, name: 'Import from Garmin' })).toBeInTheDocument();
    expect(screen.getByText(/Export Your Data/)).toBeInTheDocument();
    expect(screen.getByText(/Export File/)).toBeInTheDocument();
    const el = await input();
    expect(el).toHaveAttribute('type', 'file');
    expect(el).toHaveAttribute('accept', '.zip,.fit');
    expect(el).toHaveProperty('multiple', true);
  });

  it('uploads one file at a time with progress, then shows the counts', async () => {
    const { invalidate } = renderPage();
    const user = userEvent.setup();
    await user.upload(await input(), [file('export.zip', 1000), file('1234.zip', 50)]);

    // The first goes out as the raw body, with the CSRF header and its name; the second waits.
    expect(FakeXhr.all).toHaveLength(1);
    const first = FakeXhr.all[0]!;
    expect(first.method).toBe('POST');
    expect(first.url).toBe('/api/import/garmin?name=export.zip');
    expect(first.headers['X-Nimrag']).toBe('1');
    expect(first.withCredentials).toBe(true);
    expect((first.body as File).name).toBe('export.zip');
    expect(screen.getByText('Waiting')).toBeInTheDocument();

    first.progress(250, 1000);
    const bar = await screen.findByRole('progressbar', { name: 'export.zip: 25% uploaded' });
    expect(bar).toHaveAttribute('aria-valuenow', '250');
    expect(screen.getByText('Uploading 25%')).toBeInTheDocument();

    first.sent();
    expect(await screen.findByText(/Reading the file on your server/)).toBeInTheDocument();
    expect(FakeXhr.all).toHaveLength(1);

    first.respond(200, result());
    expect(await screen.findByText('9 new files, 3 already here.')).toBeInTheDocument();
    expect(screen.getByText('Activities 10 · Sleep 2')).toBeInTheDocument();
    expect(invalidate).toHaveBeenCalled();

    // Only now does the second one start.
    expect(FakeXhr.all).toHaveLength(2);
    expect(FakeXhr.all[1]!.url).toBe('/api/import/garmin?name=1234.zip');
    FakeXhr.all[1]!.respond(200, result({ name: '1234.zip', fit_found: 0, new: 0, already_present: 0, skipped: 1, kinds: [] }));
    expect(await screen.findByText('No FIT files found.')).toBeInTheDocument();
  });

  it('lists the problems the server named', async () => {
    renderPage();
    const user = userEvent.setup();
    await user.upload(await input(), file('export.zip'));
    FakeXhr.all[0]!.respond(200, result({ failed: 25, errors: ['export.zip/a.fit: invalid checksum', 'export.zip/b.fit: invalid checksum'] }));
    expect(await screen.findByText('25 files could not be read.')).toBeInTheDocument();
    const details = screen.getByText('Show problems').closest('details')!;
    expect(within(details).getAllByRole('listitem')).toHaveLength(2);
    expect(within(details).getByText('And 23 more.')).toBeInTheDocument();
  });

  it('refuses a file that is not a zip or a FIT file without sending it', async () => {
    renderPage();
    const user = userEvent.setup({ applyAccept: false });
    await user.upload(await input(), file('notes.txt'));
    expect(await screen.findByRole('alert')).toHaveTextContent('Only .zip and .fit files can be imported.');
    expect(FakeXhr.all).toHaveLength(0);
    expect(screen.queryByRole('button', { name: 'Try again' })).not.toBeInTheDocument();
  });

  it('says when a file is too big and lets the person try again', async () => {
    renderPage();
    const user = userEvent.setup();
    await user.upload(await input(), file('huge.zip'));
    FakeXhr.all[0]!.respond(413, { error: 'file too large' });
    expect(await screen.findByRole('alert')).toHaveTextContent('This file is too big for the server.');
    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(FakeXhr.all).toHaveLength(2);
    FakeXhr.all[1]!.respond(200, result());
    expect(await screen.findByText('9 new files, 3 already here.')).toBeInTheDocument();
  });

  it('reports a lost connection', async () => {
    renderPage();
    const user = userEvent.setup();
    await user.upload(await input(), file('a.zip'));
    act(() => FakeXhr.all[0]!.onerror?.());
    expect(await screen.findByRole('alert')).toHaveTextContent('Lost the connection to the server.');
  });

  it('takes files dropped on the page', async () => {
    renderPage();
    const zone = (await input()).closest('label')!;
    fireEvent.dragOver(zone);
    expect(zone).toHaveClass('dropzone--over');
    fireEvent.drop(zone, { dataTransfer: { files: [file('dropped.fit')] } });
    expect(zone).not.toHaveClass('dropzone--over');
    expect(await screen.findByText('dropped.fit')).toBeInTheDocument();
    expect(FakeXhr.all[0]!.url).toBe('/api/import/garmin?name=dropped.fit');
  });

  it('encodes the name and cancels a running upload, then goes on with the next', async () => {
    renderPage();
    const user = userEvent.setup();
    await user.upload(await input(), [file('my export & more.zip'), file('next.fit')]);
    expect(FakeXhr.all[0]!.url).toBe('/api/import/garmin?name=my%20export%20%26%20more.zip');
    const [cancelRunning] = screen.getAllByRole('button', { name: 'Cancel' });
    await user.click(cancelRunning!);
    expect(FakeXhr.all[0]!.aborted).toBe(true);
    expect(await screen.findByText('Cancelled')).toBeInTheDocument();
    expect(FakeXhr.all).toHaveLength(2);
    expect(FakeXhr.all[1]!.url).toBe('/api/import/garmin?name=next.fit');
    FakeXhr.all[1]!.respond(200, result({ new: 1, already_present: 0, fit_found: 1, skipped: 0 }));
    await user.click(await screen.findByRole('button', { name: 'Clear finished' }));
    expect(screen.queryByText('next.fit')).not.toBeInTheDocument();
  });
});

describe('uploadFile', () => {
  it('sends the lost session to the login handler and rejects as unauthorized', async () => {
    FakeXhr.all = [];
    vi.stubGlobal('XMLHttpRequest', FakeXhr);
    const handler = vi.fn();
    setUnauthorizedHandler(handler);
    const promise = uploadFile('/api/import/garmin?name=a.zip', file('a.zip'));
    FakeXhr.all[0]!.respond(401, { error: 'sign in required' });
    await expect(promise).rejects.toMatchObject({ name: 'UnauthorizedError', status: 401 });
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('rejects with an AbortError when the signal is already aborted', async () => {
    vi.stubGlobal('XMLHttpRequest', FakeXhr);
    const controller = new AbortController();
    controller.abort();
    await expect(uploadFile('/x', file('a.zip'), { signal: controller.signal })).rejects.toMatchObject({ name: 'AbortError' });
  });
});
