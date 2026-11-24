import type { NimragImportResponse } from '../../api/generated';
import { useUploadQueue, type QueueConfig } from './garminImport';

export const RESTORE_PATH = '/api/import/nimrag';

/** A Nimrag export is the zip of "Download my data". */
export function isRestorable(name: string): boolean {
  return /\.zip$/i.test(name);
}

export interface RestoreSummary {
  /** The first line: what came back. */
  headline: string;
  /** Other facts, one short line each. */
  notes: string[];
  /** Something was not restored: shown with the warning colour. */
  warn: boolean;
}

const count = (n: number, one: string, many: string) => `${n.toLocaleString('en-GB')} ${n === 1 ? one : many}`;

/** What the server's counts say, in short sentences. */
export function summarizeRestore(r: NimragImportResponse): RestoreSummary {
  const added = [
    r.files.added > 0 ? count(r.files.added, 'new file', 'new files') : '',
    r.workouts.added > 0 ? count(r.workouts.added, 'workout', 'workouts') : '',
    r.body_measurements.added > 0 ? count(r.body_measurements.added, 'body measurement', 'body measurements') : '',
    r.strength_set_edits.applied > 0 ? count(r.strength_set_edits.applied, 'strength edit', 'strength edits') : '',
  ].filter(Boolean);
  const notes: string[] = [];
  if (r.files.already_here > 0) notes.push(`${count(r.files.already_here, 'file was', 'files were')} already here.`);
  const alreadyEntries = r.workouts.already_here + r.body_measurements.already_here;
  if (alreadyEntries > 0) notes.push(`${count(alreadyEntries, 'workout or body measurement was', 'workouts and body measurements were')} already here.`);
  if (r.strength_set_edits.older > 0) notes.push(`${count(r.strength_set_edits.older, 'strength edit was', 'strength edits were')} kept as it is, because yours is newer.`);
  if (r.settings_applied > 0) notes.push(`${count(r.settings_applied, 'setting', 'settings')} restored.`);
  if (r.files.missing_in_export > 0) notes.push(`${count(r.files.missing_in_export, 'file was', 'files were')} not in the export.`);
  if (r.files.refused > 0) notes.push(`${count(r.files.refused, 'file', 'files')} could not be restored.`);
  if (r.settings_skipped > 0) notes.push(`${count(r.settings_skipped, 'setting was', 'settings were')} left out.`);
  const headline = added.length > 0 ? `Restored ${added.join(', ')}.` : 'Nothing new: everything in this export is already here.';
  return { headline, notes, warn: r.files.refused > 0 || r.errors.length > 0 };
}

/** True when the restore changed what the pages show. */
export function restoreChanged(r: NimragImportResponse): boolean {
  return r.files.added + r.workouts.added + r.body_measurements.added + r.strength_set_edits.applied + r.settings_applied > 0;
}

const RESTORE_QUEUE: QueueConfig<NimragImportResponse> = {
  path: RESTORE_PATH,
  accepts: isRestorable,
  refusal: 'Only the .zip from Download my data can be restored.',
  changed: restoreChanged,
};

/** The restore page's queue: the same one-at-a-time upload as the Garmin import, to another endpoint. */
export function useRestoreQueue() {
  return useUploadQueue(RESTORE_QUEUE);
}
