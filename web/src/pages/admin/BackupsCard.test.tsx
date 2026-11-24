import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { BackupsResponse } from '../../api/generated';
import { json, mockApi, renderAt } from '../body/testUtils';
import { BackupsCard, fmtIn, fmtWhen, statusLine, toRequest } from './BackupsCard';

const TZ = 'Europe/Copenhagen';
const nowS = () => Math.floor(Date.now() / 1000);

const entry = (over: Partial<BackupsResponse['backups'][number]> = {}) => ({
  name: 'nimrag-backup-20261124T020000Z',
  at: 1_795_572_000,
  files: 1200,
  files_bytes: 200 * 1024 * 1024,
  db_bytes: 13 * 1024 * 1024,
  ...over,
});

function data(over: Partial<BackupsResponse> = {}): BackupsResponse {
  return {
    settings: { enabled: true, dir: '/backups', every_s: 86_400, keep: 7 },
    source: 'saved',
    startup: null,
    status: { running: false, last_ok: null, last_error: null, next_at: nowS() + 21 * 3600 },
    backups: [],
    ...over,
  };
}

function open(initial: BackupsResponse, extra: Record<string, unknown> = {}) {
  const api = mockApi({ '/api/admin/backups': initial, ...extra });
  renderAt(<BackupsCard timezone={TZ} />, '/admin');
  return api;
}

const bodyOf = (calls: { url: string; init: RequestInit | undefined }[], method: string, url = '/api/admin/backups') =>
  JSON.parse(String(calls.find((c) => c.url === url && c.init?.method === method)?.init?.body));

afterEach(() => vi.unstubAllGlobals());

describe('Backups card: wording', () => {
  const now = Date.parse('2026-11-24T12:00:00Z');

  it('says when, how big the newest backup is and when the next one is', () => {
    const at = Math.floor(Date.parse('2026-11-24T02:00:00Z') / 1000);
    // Everything was linked from the backup before, so nothing was copied: the size is the backup's own.
    const d = data({
      status: { running: false, last_ok: { name: 'x', at, duration_ms: 900, copied_bytes: 0 }, last_error: null, next_at: Math.floor(now / 1000) + 21 * 3600 },
      backups: [entry()],
    });
    expect(statusLine(d, TZ, now)).toEqual({ text: 'Last backup today 03:00 · 213 MB · next in 21 h', error: false });
  });

  it('says Off, backing up, and shows an error as an error', () => {
    expect(statusLine(data({ status: { running: false, last_ok: null, last_error: null, next_at: null } }), TZ, now).text).toBe('Off');
    expect(statusLine(data({ status: { running: true, last_ok: null, last_error: null, next_at: null } }), TZ, now).text).toBe('Backing up…');
    const failed = statusLine(data({ status: { running: false, last_ok: null, last_error: { text: 'The server cannot write to this folder.', at: Math.floor(now / 1000) - 3600 }, next_at: null } }), TZ, now);
    expect(failed.error).toBe(true);
    expect(failed.text).toBe('The last backup failed today 12:00: The server cannot write to this folder.');
  });

  it('knows the newest backup after a restart, when nothing ran yet', () => {
    const d = data({
      status: { running: false, last_ok: null, last_error: null, next_at: Math.floor(now / 1000) + 21 * 3600 },
      backups: [entry({ at: Math.floor(Date.parse('2026-11-23T10:00:00Z') / 1000) })],
    });
    expect(statusLine(d, TZ, now).text).toBe('Last backup yesterday 11:00 · 213 MB · next in 21 h');
  });

  it('formats days, times and waits', () => {
    expect(fmtWhen(Math.floor(Date.parse('2026-11-19T06:42:00Z') / 1000), TZ, now)).toBe('19 Nov, 07:42');
    expect(fmtIn(Math.floor(now / 1000) + 40 * 60, now)).toBe('in 40 min');
    expect(fmtIn(Math.floor(now / 1000) + 3 * 86_400, now)).toBe('in 3 days');
    expect(fmtIn(Math.floor(now / 1000) - 5, now)).toBe('now');
  });

  it('builds the request, and names the field when a number is wrong', () => {
    const v = { enabled: true, dir: ' /b ', every: '86400', hours: '5', keep: '7' };
    expect(toRequest(v)).toEqual({ body: { enabled: true, dir: '/b', every_s: 86_400, keep: 7 } });
    expect(toRequest({ ...v, every: 'custom', hours: '10' })).toEqual({ body: { enabled: true, dir: '/b', every_s: 36_000, keep: 7 } });
    expect(toRequest({ ...v, every: 'custom', hours: '0' })).toMatchObject({ field: 'every_s' });
    expect(toRequest({ ...v, every: 'custom', hours: '721' })).toMatchObject({ field: 'every_s' });
    expect(toRequest({ ...v, every: 'custom', hours: '1.5' })).toMatchObject({ field: 'every_s' });
    expect(toRequest({ ...v, keep: '0' })).toMatchObject({ field: 'keep' });
    expect(toRequest({ ...v, keep: '366' })).toMatchObject({ field: 'keep' });
  });
});

describe('Backups card', () => {
  it('shows the status, the settings and the list of backups', async () => {
    const at = nowS();
    open(
      data({
        status: { running: false, last_ok: { name: 'nimrag-backup-x', at, duration_ms: 1200, copied_bytes: 5 * 1024 * 1024 }, last_error: null, next_at: at + 21 * 3600 },
        backups: [entry({ name: 'nimrag-backup-20261124T020000Z' }), entry({ name: 'nimrag-backup-20261123T020000Z', files_bytes: null, db_bytes: null })],
      }),
    );
    expect(await screen.findByText(/Last backup today \d\d:\d\d · 213 MB · next in 21 h/)).toBeInTheDocument();
    expect(screen.getByLabelText('Folder')).toHaveValue('/backups');
    expect(screen.getByLabelText('How often')).toHaveValue('86400');
    expect(screen.getByLabelText('Keep')).toHaveValue(7);
    expect(screen.getByRole('switch', { name: 'Back up automatically' })).toBeChecked();
    expect(screen.getByText('Set here, on this page.')).toBeInTheDocument();
    const list = screen.getByRole('list', { name: 'Backups in the folder' });
    const rows = within(list).getAllByRole('listitem');
    expect(rows).toHaveLength(2);
    expect(within(rows[0]!).getByText('nimrag-backup-20261124T020000Z')).toBeInTheDocument();
    expect(within(rows[0]!).getByText('213 MB')).toBeInTheDocument();
    expect(within(rows[1]!).getByText('size unknown')).toBeInTheDocument();
    expect(screen.getByText(/nimrag-server restore --from/)).toBeInTheDocument();
  });

  it('starts from the start-up options when nothing is saved, and from defaults when there are none', async () => {
    open(data({ settings: null, source: 'startup', startup: { dir: '/srv/backups', every_s: 43_200, keep: 3 }, status: { running: false, last_ok: null, last_error: null, next_at: nowS() + 60 } }));
    expect(await screen.findByLabelText('Folder')).toHaveValue('/srv/backups');
    expect(screen.getByLabelText('How often')).toHaveValue('43200');
    expect(screen.getByLabelText('Keep')).toHaveValue(3);
    expect(screen.getByText('Set at server start. Saving here replaces it.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Use start-up settings' })).not.toBeInTheDocument();
  });

  it('defaults to off, daily and 7 when nothing is set', async () => {
    open(data({ settings: null, source: 'none', status: { running: false, last_ok: null, last_error: null, next_at: null } }));
    expect(await screen.findByLabelText('Folder')).toHaveValue('');
    expect(screen.getByRole('switch', { name: 'Back up automatically' })).not.toBeChecked();
    expect(screen.getByLabelText('How often')).toHaveValue('86400');
    expect(screen.getByLabelText('Keep')).toHaveValue(7);
    expect(screen.getByText('Off')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Back up now' })).toBeDisabled();
  });

  it('saves the folder, the interval and the keep count', async () => {
    const saved = data({ settings: { enabled: true, dir: '/mnt/nas/nimrag', every_s: 604_800, keep: 12 } });
    const { calls } = open(data({ settings: null, source: 'none', status: { running: false, last_ok: null, last_error: null, next_at: null } }), { 'PUT /api/admin/backups': saved });
    const u = userEvent.setup();
    await u.click(await screen.findByRole('switch', { name: 'Back up automatically' }));
    await u.type(screen.getByLabelText('Folder'), '/mnt/nas/nimrag');
    await u.selectOptions(screen.getByLabelText('How often'), 'Weekly');
    await u.clear(screen.getByLabelText('Keep'));
    await u.type(screen.getByLabelText('Keep'), '12');
    await u.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Saved.')).toBeInTheDocument();
    expect(bodyOf(calls, 'PUT')).toEqual({ enabled: true, dir: '/mnt/nas/nimrag', every_s: 604_800, keep: 12 });
    expect(screen.getByText('Set here, on this page.')).toBeInTheDocument();
  });

  it('shows the server\'s refusal under the folder field', async () => {
    const text = 'This folder does not exist on the server. With Docker, mount a folder into the container first, for example /backups.';
    open(data(), { 'PUT /api/admin/backups': () => json({ error: text, field: 'dir' }, { status: 422 }) });
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Save' }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(text);
    expect(screen.getByLabelText('Folder')).toHaveAccessibleDescription(text);
    expect(screen.getByLabelText('Folder')).toBeInvalid();
    // Typing again clears it.
    await u.type(screen.getByLabelText('Folder'), 'x');
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('takes whole hours for Custom and refuses others without asking the server', async () => {
    const { calls } = open(data(), { 'PUT /api/admin/backups': data({ settings: { enabled: true, dir: '/backups', every_s: 36_000, keep: 7 } }) });
    const u = userEvent.setup();
    await u.selectOptions(await screen.findByLabelText('How often'), 'Custom');
    const hours = screen.getByLabelText('Hours between backups');
    expect(hours).toHaveValue(24);
    await u.clear(hours);
    await u.type(hours, '0');
    await u.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Use a whole number of hours from 1 to 720.');
    expect(calls.some((c) => c.init?.method === 'PUT')).toBe(false);
    await u.clear(hours);
    await u.type(hours, '10');
    await u.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Saved.')).toBeInTheDocument();
    expect(bodyOf(calls, 'PUT').every_s).toBe(36_000);
  });

  it('starts a backup now and waits while it runs', async () => {
    const running = data({ status: { running: true, last_ok: null, last_error: null, next_at: nowS() + 3600 } });
    const { calls } = open(data(), { 'POST /api/admin/backups/run': running });
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Back up now' }));
    expect(await screen.findByText('Backing up…')).toBeInTheDocument();
    expect(calls.some((c) => c.url === '/api/admin/backups/run' && c.init?.method === 'POST')).toBe(true);
    expect(screen.getByRole('button', { name: 'Back up now' })).toBeDisabled();
  });

  it('shows the 409 when a backup is already running', async () => {
    open(data(), { 'POST /api/admin/backups/run': () => json({ error: 'A backup is already running.' }, { status: 409 }) });
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Back up now' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('A backup is already running.');
  });

  it('goes back to the start-up settings', async () => {
    const back = data({ settings: null, source: 'startup', startup: { dir: '/srv/backups', every_s: 86_400, keep: 7 } });
    const { calls } = open(data({ startup: { dir: '/srv/backups', every_s: 86_400, keep: 7 } }), { 'DELETE /api/admin/backups': back });
    const u = userEvent.setup();
    await u.click(await screen.findByRole('button', { name: 'Use start-up settings' }));
    expect(await screen.findByText('Set at server start. Saving here replaces it.')).toBeInTheDocument();
    expect(calls.some((c) => c.init?.method === 'DELETE')).toBe(true);
    expect(screen.getByLabelText('Folder')).toHaveValue('/srv/backups');
  });

  it('shows an error that stopped a backup in the danger tone', async () => {
    open(data({ status: { running: false, last_ok: null, last_error: { text: 'The backup folder cannot be read.', at: nowS() }, next_at: null } }));
    expect(await screen.findByRole('alert')).toHaveTextContent('The last backup failed today');
    expect(screen.getByRole('alert')).toHaveTextContent('The backup folder cannot be read.');
  });
});
