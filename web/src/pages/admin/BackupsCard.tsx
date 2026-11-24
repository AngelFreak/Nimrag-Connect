import { useState, type FormEvent } from 'react';
import { ApiError } from '../../api/client';
import type { BackupSettings, BackupsResponse } from '../../api/generated';
import type { BackupEntry } from '../../api/generated/BackupEntry';
import { Button, Card, CardHeader, ErrorState, SelectField, Skeleton, TextField, Toggle } from '../../components';
import { fmtBytes, fmtDateTime } from '../today/format';
import { errorMessage } from '../system/errors';
import { useBackups, useRunBackup, useSaveBackups, useUseStartupBackups } from './queries';

const HOUR = 3600;
const PRESETS: { seconds: number; label: string }[] = [
  { seconds: 6 * HOUR, label: 'Every 6 hours' },
  { seconds: 12 * HOUR, label: 'Every 12 hours' },
  { seconds: 24 * HOUR, label: 'Daily' },
  { seconds: 48 * HOUR, label: 'Every 2 days' },
  { seconds: 168 * HOUR, label: 'Weekly' },
];
const CUSTOM = 'custom';
const MAX_HOURS = 720;
const MAX_KEEP = 365;

interface FormValues {
  enabled: boolean;
  dir: string;
  every: string;
  hours: string;
  keep: string;
}

/** The form's first values: what is saved, else the start-up options, else plain defaults. */
export function initialValues(data: BackupsResponse): FormValues {
  const base: BackupSettings = data.settings ?? (data.startup ? { enabled: true, dir: data.startup.dir, every_s: data.startup.every_s, keep: data.startup.keep } : { enabled: false, dir: '', every_s: 24 * HOUR, keep: 7 });
  const preset = PRESETS.find((p) => p.seconds === base.every_s);
  return {
    enabled: base.enabled,
    dir: base.dir,
    every: preset ? String(preset.seconds) : CUSTOM,
    hours: String(Math.min(MAX_HOURS, Math.max(1, Math.ceil(base.every_s / HOUR)))),
    keep: String(base.keep),
  };
}

function wholeNumber(text: string, min: number, max: number): number | null {
  if (!/^\d+$/.test(text.trim())) return null;
  const n = Number(text);
  return n >= min && n <= max ? n : null;
}

export type Field = 'dir' | 'every_s' | 'keep';

/** The request for the form, or the first problem in the form's own words. */
export function toRequest(v: FormValues): { body: BackupSettings } | { field: Field; text: string } {
  let every_s: number;
  if (v.every === CUSTOM) {
    const hours = wholeNumber(v.hours, 1, MAX_HOURS);
    if (hours === null) return { field: 'every_s', text: `Use a whole number of hours from 1 to ${MAX_HOURS}.` };
    every_s = hours * HOUR;
  } else {
    every_s = Number(v.every);
  }
  const keep = wholeNumber(v.keep, 1, MAX_KEEP);
  if (keep === null) return { field: 'keep', text: `Keep between 1 and ${MAX_KEEP} backups.` };
  return { body: { enabled: v.enabled, dir: v.dir.trim(), every_s, keep } };
}

/** "today 03:00", "yesterday 03:00" or "19 Nov, 03:00" in the admin's time zone. */
export function fmtWhen(unix: number, timezone: string, nowMs = Date.now()): string {
  const day = (ms: number) => {
    try {
      return new Intl.DateTimeFormat('en-CA', { timeZone: timezone }).format(new Date(ms));
    } catch {
      return new Intl.DateTimeFormat('en-CA').format(new Date(ms));
    }
  };
  const ms = unix * 1000;
  const full = fmtDateTime(new Date(ms).toISOString(), timezone) ?? '';
  const clock = full.split(', ')[1] ?? full;
  if (day(ms) === day(nowMs)) return `today ${clock}`;
  if (day(ms) === day(nowMs - 86_400_000)) return `yesterday ${clock}`;
  return full;
}

/** "in 21 h", "in 40 min", "in 3 days" for a time ahead. */
export function fmtIn(unix: number, nowMs = Date.now()): string {
  const s = Math.round(unix - nowMs / 1000);
  if (s <= 60) return 'now';
  if (s < HOUR) return `in ${Math.ceil(s / 60)} min`;
  if (s < 48 * HOUR) return `in ${Math.round(s / HOUR)} h`;
  return `in ${Math.round(s / (24 * HOUR))} days`;
}

/** The status line, and whether it is an error. */
export function statusLine(data: BackupsResponse, timezone: string, nowMs = Date.now()): { text: string; error: boolean } {
  const { status } = data;
  if (status.running) return { text: 'Backing up…', error: false };
  if (status.last_error) return { text: `The last backup failed ${fmtWhen(status.last_error.at, timezone, nowMs)}: ${status.last_error.text}`, error: true };
  const newest = data.backups[0];
  const lastAt = status.last_ok?.at ?? newest?.at;
  const parts: string[] = [];
  const off = status.next_at === null;
  if (off) parts.push('Off');
  if (lastAt !== undefined) {
    parts.push(`Last backup ${fmtWhen(lastAt, timezone, nowMs)}`);
    // The backup's whole size: "copied" would say 0 B when everything was linked from the one before.
    if (newest && (newest.db_bytes !== null || newest.files_bytes !== null)) parts.push(sizeOf(newest));
  } else if (!off) {
    parts.push('No backup yet');
  }
  if (status.next_at !== null) parts.push(`next ${fmtIn(status.next_at, nowMs)}`);
  return { text: parts.join(' · '), error: false };
}

function sizeOf(b: BackupEntry): string {
  if (b.db_bytes === null && b.files_bytes === null) return 'size unknown';
  return fmtBytes((b.db_bytes ?? 0) + (b.files_bytes ?? 0)) ?? 'size unknown';
}

function FieldError({ text }: { text: string }) {
  return (
    <span role="alert" style={{ color: 'var(--heart)' }}>
      {text}
    </span>
  );
}

interface FormProps {
  data: BackupsResponse;
  onSaved: (message: string | null) => void;
}

function BackupsForm({ data, onSaved }: FormProps) {
  const save = useSaveBackups();
  const [values, setValues] = useState(() => initialValues(data));
  const [problem, setProblem] = useState<{ field: Field; text: string } | null>(null);
  const set = (patch: Partial<FormValues>) => {
    onSaved(null);
    setProblem(null);
    save.reset();
    setValues((v) => ({ ...v, ...patch }));
  };

  // The server's refusal names the field it belongs to.
  const refused = save.error instanceof ApiError && save.error.status === 422 ? (save.error.body as { field?: Field } | undefined)?.field : undefined;
  const serverProblem = save.isError && !problem ? { field: refused, text: errorMessage(save.error) } : null;
  const shown = (field: Field): string | null => (problem?.field === field ? problem.text : serverProblem?.field === field ? serverProblem.text : null);
  const general = serverProblem && !serverProblem.field ? serverProblem.text : null;

  function submit(e: FormEvent) {
    e.preventDefault();
    if (save.isPending) return;
    const req = toRequest(values);
    if ('field' in req) {
      setProblem(req);
      return;
    }
    save.mutate(req.body, { onSuccess: () => onSaved('Saved.') });
  }

  const dirError = shown('dir');
  const everyError = shown('every_s');
  const keepError = shown('keep');
  return (
    <form onSubmit={submit} noValidate aria-label="Backup settings" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <Toggle label="Back up automatically" checked={values.enabled} onChange={(e) => set({ enabled: e.target.checked })} />
      <TextField
        label="Folder"
        name="backup-dir"
        autoComplete="off"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        placeholder="/backups"
        style={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace' }}
        value={values.dir}
        onChange={(e) => set({ dir: e.target.value })}
        aria-invalid={dirError ? true : undefined}
        hint={dirError ? <FieldError text={dirError} /> : 'A folder on the server, ideally on another disk. With Docker, mount it into the container first.'}
      />
      <SelectField label="How often" value={values.every} onChange={(e) => set({ every: e.target.value })} aria-invalid={everyError && values.every !== CUSTOM ? true : undefined}>
        {PRESETS.map((p) => (
          <option key={p.seconds} value={p.seconds}>
            {p.label}
          </option>
        ))}
        <option value={CUSTOM}>Custom</option>
      </SelectField>
      {values.every === CUSTOM ? (
        <TextField
          label="Hours between backups"
          name="backup-hours"
          type="number"
          inputMode="numeric"
          min={1}
          max={MAX_HOURS}
          value={values.hours}
          onChange={(e) => set({ hours: e.target.value })}
          aria-invalid={everyError ? true : undefined}
          hint={everyError ? <FieldError text={everyError} /> : `Whole hours, 1 to ${MAX_HOURS}.`}
        />
      ) : everyError ? (
        <FieldError text={everyError} />
      ) : null}
      <TextField
        label="Keep"
        name="backup-keep"
        type="number"
        inputMode="numeric"
        min={1}
        max={MAX_KEEP}
        value={values.keep}
        onChange={(e) => set({ keep: e.target.value })}
        aria-invalid={keepError ? true : undefined}
        hint={keepError ? <FieldError text={keepError} /> : 'The newest backups to keep. Older ones are removed after a new one succeeds.'}
      />
      {general ? (
        <p className="status-msg status-msg--error" role="alert">
          {general}
        </p>
      ) : null}
      <Button type="submit" size="lg" variant="primary" disabled={save.isPending} aria-busy={save.isPending}>
        {save.isPending ? 'Saving' : 'Save'}
      </Button>
    </form>
  );
}

function BackupList({ backups, timezone }: { backups: BackupEntry[]; timezone: string }) {
  if (backups.length === 0) return <p className="stat__sub">No backups in this folder yet.</p>;
  return (
    <ul className="user-list" aria-label="Backups in the folder">
      {backups.map((b) => (
        <li key={b.name} className="user-row">
          <p className="user-row__name">
            <span>{fmtDateTime(new Date(b.at * 1000).toISOString(), timezone) ?? b.name}</span>
            <span className="user-row__facts" style={{ fontWeight: 400 }}>
              {sizeOf(b)}
            </span>
          </p>
          <p className="user-row__facts" style={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', overflowWrap: 'anywhere' }}>
            {b.name}
          </p>
        </li>
      ))}
    </ul>
  );
}

function sourceLine(data: BackupsResponse): string {
  if (data.source === 'saved') return 'Set here, on this page.';
  if (data.source === 'startup') return 'Set at server start. Saving here replaces it.';
  return 'Not set up yet.';
}

/** Admin: scheduled backups into a folder on the server, set here instead of at server start. */
export function BackupsCard({ timezone }: { timezone: string }) {
  const query = useBackups();
  const run = useRunBackup();
  const useStartup = useUseStartupBackups();
  const [done, setDone] = useState<string | null>(null);
  const data = query.data;
  const folder = data ? (data.settings ? data.settings.dir : (data.startup?.dir ?? '')) : '';
  const line = data ? statusLine(data, timezone) : null;

  return (
    <Card padding="lg" aria-labelledby="backups-title">
      <CardHeader title="Backups" id="backups-title" aside="Copies of the data on the server" />
      {query.isPending ? <Skeleton height={20} lines={3} /> : null}
      {query.isError && !data ? (
        <ErrorState title="Could not load the backups" action={<Button onClick={() => void query.refetch()}>Try again</Button>}>
          {errorMessage(query.error)}
        </ErrorState>
      ) : null}
      {data && line ? (
        <>
          <p className={line.error ? 'status-msg status-msg--error' : 'stat__sub'} role={line.error ? 'alert' : undefined}>
            {line.text}
          </p>
          <Button
            disabled={!folder || data.status.running || run.isPending}
            aria-busy={data.status.running || run.isPending}
            onClick={() => {
              setDone(null);
              run.mutate();
            }}
          >
            Back up now
          </Button>
          {run.isError ? (
            <p className="status-msg status-msg--error" role="alert">
              {errorMessage(run.error)}
            </p>
          ) : null}
          <BackupsForm key={`${data.source}:${JSON.stringify(data.settings)}`} data={data} onSaved={setDone} />
          {done ? (
            <p className="status-msg status-msg--ok" role="status">
              {done}
            </p>
          ) : null}
          <p className="stat__sub">
            {sourceLine(data)}
            {data.source === 'saved' && data.startup ? ' ' : ''}
          </p>
          {data.source === 'saved' && data.startup ? (
            <Button
              disabled={useStartup.isPending}
              onClick={() => {
                setDone(null);
                useStartup.mutate();
              }}
            >
              Use start-up settings
            </Button>
          ) : null}
          {useStartup.isError ? (
            <p className="status-msg status-msg--error" role="alert">
              {errorMessage(useStartup.error)}
            </p>
          ) : null}
          <h3 className="stat__label" style={{ margin: '8px 0 0' }}>
            Backups in the folder
          </h3>
          <BackupList backups={data.backups} timezone={timezone} />
          <p className="stat__sub">
            To restore, stop the server and run <code>nimrag-server restore --from {'<folder>/<backup>'} --data-dir {'<new folder>'}</code> (see the runbook).
          </p>
        </>
      ) : null}
    </Card>
  );
}
