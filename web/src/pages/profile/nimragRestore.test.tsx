import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../../App';
import { setUnauthorizedHandler } from '../../api/client';
import type { NimragImportResponse, SettingsResponse } from '../../api/generated';
import { mockApi } from '../body/testUtils';
import { isRestorable, summarizeRestore } from './nimragRestore';

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
  intensity_goal_min: 150,
  home_cards: { shown: [], hidden: [] } as unknown as SettingsResponse['home_cards'],
  sample: false,
};
const me = { authenticated: true, username: 'demo', role: 'user', timezone: 'Europe/Copenhagen', sample: false };

const result = (over: Partial<NimragImportResponse> = {}): NimragImportResponse => ({
  name: 'nimrag-export.zip',
  files: { added: 3, already_here: 1, missing_in_export: 0, refused: 0 },
  settings_applied: 4,
  settings_skipped: 0,
  workouts: { added: 2, already_here: 0 },
  body_measurements: { added: 1, already_here: 1 },
  strength_set_edits: { applied: 1, older: 0 },
  errors: [],
  ...over,
});

const nothingNew = (): NimragImportResponse =>
  result({
    files: { added: 0, already_here: 4, missing_in_export: 0, refused: 0 },
    settings_applied: 0,
    workouts: { added: 0, already_here: 2 },
    body_measurements: { added: 0, already_here: 2 },
    strength_set_edits: { applied: 0, older: 1 },
  });

function renderPage(path = '/profile/restore') {
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
const input = () => screen.findByLabelText('Nimrag export file') as Promise<HTMLInputElement>;

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  setUnauthorizedHandler(null);
});

describe('Restore from a Nimrag export: the words', () => {
  it('takes zips only, whatever the case', () => {
    for (const ok of ['nimrag-export-demo-2026-11-24.zip', 'Export (1).ZIP']) expect(isRestorable(ok)).toBe(true);
    for (const no of ['ride.fit', 'a.zip.txt', 'zip']) expect(isRestorable(no)).toBe(false);
  });

  it('says what came back, what was already here and what was left out', () => {
    const s = summarizeRestore(result());
    expect(s.headline).toBe('Restored 3 new files, 2 workouts, 1 body measurement, 1 strength edit.');
    expect(s.notes).toEqual(['1 file was already here.', '1 workout or body measurement was already here.', '4 settings restored.']);
    expect(s.warn).toBe(false);

    const same = summarizeRestore(nothingNew());
    expect(same.headline).toBe('Nothing new: everything in this export is already here.');
    expect(same.notes).toContain('4 files were already here.');
    expect(same.notes).toContain('1 strength edit was kept as it is, because yours is newer.');

    const bad = summarizeRestore(
      result({ files: { added: 0, already_here: 0, missing_in_export: 2, refused: 1 }, settings_skipped: 1, errors: ['originals/a.fit: the content does not match its SHA-256'] }),
    );
    expect(bad.warn).toBe(true);
    expect(bad.notes).toContain('1 file could not be restored.');
    expect(bad.notes).toContain('2 files were not in the export.');
    expect(bad.notes).toContain('1 setting was left out.');
  });
});

describe('Restore from a Nimrag export: the page', () => {
  it('is linked from the Account list on the profile page', async () => {
    renderPage('/profile');
    const link = await screen.findByRole('link', { name: /^Restore from a Nimrag export/ });
    expect(link).toHaveAttribute('href', '/profile/restore');
    expect(link).toHaveTextContent('A zip you downloaded from Nimrag');
  });

  it('offers a normal file input for one zip', async () => {
    renderPage();
    expect(await screen.findByRole('heading', { level: 1, name: 'Restore from a Nimrag export' })).toBeInTheDocument();
    expect(screen.getByText(/Download my data/)).toBeInTheDocument();
    const el = await input();
    expect(el).toHaveAttribute('type', 'file');
    expect(el).toHaveAttribute('accept', '.zip');
  });

  it('uploads the zip with progress, then shows the summary and refreshes the pages', async () => {
    const { invalidate } = renderPage();
    const user = userEvent.setup();
    await user.upload(await input(), file('nimrag-export.zip', 1000));

    expect(FakeXhr.all).toHaveLength(1);
    const xhr = FakeXhr.all[0]!;
    expect(xhr.method).toBe('POST');
    expect(xhr.url).toBe('/api/import/nimrag?name=nimrag-export.zip');
    expect(xhr.headers['X-Nimrag']).toBe('1');
    expect(xhr.withCredentials).toBe(true);
    expect((xhr.body as File).name).toBe('nimrag-export.zip');

    xhr.progress(500, 1000);
    expect(await screen.findByRole('progressbar', { name: 'nimrag-export.zip: 50% uploaded' })).toBeInTheDocument();
    xhr.sent();
    expect(await screen.findByText(/Reading the file on your server/)).toBeInTheDocument();

    xhr.respond(200, result());
    expect(await screen.findByText('Restored 3 new files, 2 workouts, 1 body measurement, 1 strength edit.')).toBeInTheDocument();
    expect(screen.getByText('4 settings restored.')).toBeInTheDocument();
    expect(invalidate).toHaveBeenCalled();
  });

  it('does not refresh anything when nothing was new', async () => {
    const { invalidate } = renderPage();
    const user = userEvent.setup();
    await user.upload(await input(), file('again.zip'));
    FakeXhr.all[0]!.respond(200, nothingNew());
    expect(await screen.findByText('Nothing new: everything in this export is already here.')).toBeInTheDocument();
    expect(invalidate).not.toHaveBeenCalled();
  });

  it('lists the problems the server named', async () => {
    renderPage();
    const user = userEvent.setup();
    await user.upload(await input(), file('export.zip'));
    FakeXhr.all[0]!.respond(200, result({ files: { added: 2, already_here: 0, missing_in_export: 0, refused: 1 }, errors: ['originals/a.fit: the content does not match its SHA-256'] }));
    expect(await screen.findByText('1 file could not be restored.')).toBeInTheDocument();
    expect(screen.getByText('Show problems')).toBeInTheDocument();
    expect(screen.getByText('originals/a.fit: the content does not match its SHA-256')).toBeInTheDocument();
  });

  it('shows the server\'s reason when it is not a Nimrag export, and lets the person try again', async () => {
    renderPage();
    const user = userEvent.setup();
    await user.upload(await input(), file('other.zip'));
    FakeXhr.all[0]!.respond(422, { error: 'This is not a Nimrag export' });
    expect(await screen.findByRole('alert')).toHaveTextContent('This is not a Nimrag export');
    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(FakeXhr.all).toHaveLength(2);
  });

  it('refuses a file that is not a zip without sending it', async () => {
    renderPage();
    const user = userEvent.setup({ applyAccept: false });
    await user.upload(await input(), file('ride.fit'));
    expect(await screen.findByRole('alert')).toHaveTextContent('Only the .zip from Download my data can be restored.');
    expect(FakeXhr.all).toHaveLength(0);
    expect(screen.queryByRole('button', { name: 'Try again' })).not.toBeInTheDocument();
  });
});
