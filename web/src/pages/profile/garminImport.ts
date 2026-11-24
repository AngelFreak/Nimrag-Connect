import { useCallback, useEffect, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { ApiError, NetworkError, uploadFile } from '../../api/client';
import type { GarminImportResponse } from '../../api/generated';

/** The server takes zips (one activity, or a whole account export) and plain FIT files. */
export function isImportable(name: string): boolean {
  return /\.(zip|fit)$/i.test(name);
}

const KIND_LABELS: Record<string, string> = {
  activity: 'Activities',
  monitoring: 'Daily monitoring',
  sleep: 'Sleep',
  metrics: 'Metrics',
  other: 'Other FIT files',
};

export function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

const count = (n: number, one: string, many: string) => `${n.toLocaleString('en-GB')} ${n === 1 ? one : many}`;

export interface ResultSummary {
  /** The first line: what was added. */
  headline: string;
  /** "Activities 10 · Sleep 2", or null when the server named no kinds. */
  kinds: string | null;
  /** Other facts, one short line each. */
  notes: string[];
  /** The file held no FIT, or something failed: shown with the warning colour. */
  warn: boolean;
}

/** What the server's counts say, in short sentences. */
export function summarizeResult(r: GarminImportResponse): ResultSummary {
  const notes: string[] = [];
  if (r.skipped > 0) notes.push(`${count(r.skipped, 'file was', 'files were')} skipped because ${r.skipped === 1 ? 'it is' : 'they are'} not FIT.`);
  if (r.failed > 0) notes.push(`${count(r.failed, 'file', 'files')} could not be read.`);
  const kinds = r.kinds.length > 0 ? r.kinds.map((k) => `${kindLabel(k.kind)} ${k.count.toLocaleString('en-GB')}`).join(' · ') : null;
  if (r.fit_found === 0) {
    return { headline: 'No FIT files found.', kinds: null, notes, warn: true };
  }
  const headline =
    r.new === 0
      ? `Nothing new: ${count(r.already_present, 'file is', 'files are')} already here.`
      : `${count(r.new, 'new file', 'new files')}${r.already_present > 0 ? `, ${r.already_present.toLocaleString('en-GB')} already here` : ''}.`;
  return { headline, kinds, notes, warn: r.failed > 0 };
}

/** Plain words for a failed upload: the server's own message, with the common cases made friendlier. */
export function importErrorMessage(error: unknown): string {
  if (error instanceof NetworkError) return 'Lost the connection to the server. Try this file again.';
  if (error instanceof ApiError) {
    if (error.status === 413) return 'This file is too big for the server.';
    if (error.status === 429) return 'The server is busy with other imports. Try again in a minute.';
    if (error.status === 403) return 'Only accounts with data can import files.';
    return error.message;
  }
  return 'Something went wrong. Try this file again.';
}

export type ItemStatus = 'waiting' | 'uploading' | 'reading' | 'done' | 'failed' | 'cancelled';

export interface ImportItem<R = GarminImportResponse> {
  id: number;
  file: File;
  status: ItemStatus;
  loaded: number;
  result?: R;
  error?: string;
}

export const IMPORT_PATH = '/api/import/garmin';

/** What one upload page sends where, and which files it takes. Keep it a constant: the queue depends on it. */
export interface QueueConfig<R> {
  /** The endpoint; the file goes as the raw body with `?name=`. */
  path: string;
  /** True for a file name the endpoint takes. */
  accepts: (name: string) => boolean;
  /** Shown for a file the page does not take (it is never sent). */
  refusal: string;
  /** True when the answer changed what the pages show. */
  changed: (result: R) => boolean;
}

const GARMIN_QUEUE: QueueConfig<GarminImportResponse> = {
  path: IMPORT_PATH,
  accepts: isImportable,
  refusal: 'Only .zip and .fit files can be imported.',
  changed: (r) => r.new > 0,
};

/** The Garmin import page's queue. */
export function useImportQueue() {
  return useUploadQueue(GARMIN_QUEUE);
}

/**
 * The files the person picked, uploaded one at a time, in order. A file is read on the server
 * after its last byte arrives (a whole export takes minutes), then the next one starts. Leaving
 * the page or closing the tab while one is uploading stops it; the server cleans up what it got.
 */
export function useUploadQueue<R>(config: QueueConfig<R>) {
  const client = useQueryClient();
  const [items, setItems] = useState<ImportItem<R>[]>([]);
  const controllers = useRef(new Map<number, AbortController>());
  const nextId = useRef(1);
  const busy = items.some((i) => i.status === 'uploading' || i.status === 'reading');

  const update = useCallback((id: number, patch: Partial<ImportItem<R>>) => {
    setItems((list) => list.map((i) => (i.id === id ? { ...i, ...patch } : i)));
  }, []);

  const add = useCallback((files: Iterable<File>) => {
    const added: ImportItem<R>[] = [];
    for (const file of files) {
      const id = nextId.current++;
      added.push(
        config.accepts(file.name)
          ? { id, file, status: 'waiting', loaded: 0 }
          : { id, file, status: 'failed', loaded: 0, error: config.refusal },
      );
    }
    if (added.length > 0) setItems((list) => [...list, ...added]);
  }, [config]);

  const retry = useCallback((id: number) => update(id, { status: 'waiting', loaded: 0, error: undefined }), [update]);

  const cancel = useCallback(
    (id: number) => {
      const controller = controllers.current.get(id);
      if (controller) controller.abort();
      else update(id, { status: 'cancelled' });
    },
    [update],
  );

  const clearFinished = useCallback(() => {
    setItems((list) => list.filter((i) => i.status === 'waiting' || i.status === 'uploading' || i.status === 'reading'));
  }, []);

  // Start the next waiting file when none is running.
  useEffect(() => {
    if (busy) return;
    const next = items.find((i) => i.status === 'waiting');
    if (!next || controllers.current.has(next.id)) return;
    const controller = new AbortController();
    controllers.current.set(next.id, controller);
    update(next.id, { status: 'uploading', loaded: 0 });
    uploadFile<R>(`${config.path}?name=${encodeURIComponent(next.file.name)}`, next.file, {
      signal: controller.signal,
      onProgress: (loaded) => update(next.id, { loaded }),
      onSent: () => update(next.id, { status: 'reading', loaded: next.file.size }),
    })
      .then((result) => {
        update(next.id, { status: 'done', result });
        // New files change what every page shows.
        if (config.changed(result)) void client.invalidateQueries();
      })
      .catch((error: unknown) => {
        if (error instanceof DOMException && error.name === 'AbortError') update(next.id, { status: 'cancelled' });
        else update(next.id, { status: 'failed', error: importErrorMessage(error) });
      })
      .finally(() => controllers.current.delete(next.id));
  }, [items, busy, client, config, update]);

  // Leaving the page ends the running upload.
  useEffect(() => {
    const running = controllers.current;
    return () => {
      for (const controller of running.values()) controller.abort();
    };
  }, []);

  // A closing tab would lose the upload: ask first.
  useEffect(() => {
    if (!busy) return;
    const ask = (e: BeforeUnloadEvent) => {
      e.preventDefault();
    };
    window.addEventListener('beforeunload', ask);
    return () => window.removeEventListener('beforeunload', ask);
  }, [busy]);

  return { items, busy, add, retry, cancel, clearFinished };
}
