import { useState, type ChangeEvent, type DragEvent, type ReactNode } from 'react';
import { Button, Card, CardHeader, ImportIcon, ProgressBar } from '../../components';
import { fmtBytes } from '../today/format';
import { SubPage } from './common';
import type { GarminImportResponse } from '../../api/generated';
import { isImportable, summarizeResult, useImportQueue, type ImportItem } from './garminImport';

function Explanation() {
  return (
    <Card padding="lg" aria-labelledby="import-kinds">
      <CardHeader title="What to pick" id="import-kinds" />
      <div className="form">
        <p className="card__intro">
          <strong>One activity.</strong> In Garmin Connect, open the activity and choose Export File (or Export Original). You get a zip named like{' '}
          <code>12345678901.zip</code>.
        </p>
        <p className="card__intro">
          <strong>Everything.</strong> On garmin.com, open your account, then Data Management, then Export Your Data. Garmin emails you a link to one big zip. It
          can be several GB.
        </p>
        <p className="card__intro">Pick the zip as it is. Do not unzip it. A single .fit file works too.</p>
      </div>
    </Card>
  );
}

function Result({ r }: { r: GarminImportResponse }) {
  const s = summarizeResult(r);
  return (
    <div className="import-item__result">
      <p className={s.warn ? 'import-item__headline import-item__headline--warn' : 'import-item__headline'} role="status">
        {s.headline}
      </p>
      {s.kinds ? <p className="import-item__line">{s.kinds}</p> : null}
      {s.notes.map((n) => (
        <p key={n} className="import-item__line">
          {n}
        </p>
      ))}
      {r.errors.length > 0 ? (
        <details className="import-item__errors">
          <summary>Show problems</summary>
          <ul>
            {r.errors.map((e, i) => (
              <li key={i}>{e}</li>
            ))}
          </ul>
          {r.failed > r.errors.length ? <p className="import-item__line">And {(r.failed - r.errors.length).toLocaleString('en-GB')} more.</p> : null}
        </details>
      ) : null}
    </div>
  );
}

/** One file of an upload queue: its progress, then `result` (what the page makes of the server's answer), or why it failed. */
export function QueueItem<R>({
  item,
  accepts,
  result,
  onCancel,
  onRetry,
}: {
  item: ImportItem<R>;
  accepts: (name: string) => boolean;
  result: (r: R) => ReactNode;
  onCancel: () => void;
  onRetry: () => void;
}) {
  const { file, status } = item;
  const pct = file.size > 0 ? Math.min(100, Math.floor((item.loaded / file.size) * 100)) : 100;
  // A file of the wrong kind never reached the server, so trying it again would change nothing.
  const canRetry = status === 'cancelled' || (status === 'failed' && accepts(file.name));
  return (
    <li className="import-item" data-status={status}>
      <div className="import-item__head">
        <span className="import-item__name">{file.name}</span>
        <span className="import-item__size">{fmtBytes(file.size)}</span>
      </div>
      {status === 'waiting' ? <p className="import-item__line">Waiting</p> : null}
      {status === 'uploading' ? (
        <>
          <ProgressBar value={item.loaded} max={file.size} label={`${file.name}: ${pct}% uploaded`} color="var(--accent)" height={8} />
          <p className="import-item__line">Uploading {pct}%</p>
        </>
      ) : null}
      {status === 'reading' ? (
        <>
          <ProgressBar value={file.size} max={file.size} label={`${file.name}: uploaded, now being read`} color="var(--energy)" height={8} />
          <p className="import-item__line" role="status">
            Reading the file on your server. A big export takes a few minutes.
          </p>
        </>
      ) : null}
      {status === 'done' && item.result ? result(item.result) : null}
      {status === 'failed' ? (
        <p className="status-msg status-msg--error" role="alert">
          {item.error}
        </p>
      ) : null}
      {status === 'cancelled' ? <p className="import-item__line">Cancelled</p> : null}
      {status === 'waiting' || status === 'uploading' ? (
        <div className="import-item__actions">
          <Button onClick={onCancel}>Cancel</Button>
        </div>
      ) : null}
      {canRetry ? (
        <div className="import-item__actions">
          <Button onClick={onRetry}>Try again</Button>
        </div>
      ) : null}
    </li>
  );
}

/**
 * Profile, Import from Garmin: pick or drop the files exported from Garmin (a single activity's
 * zip, the whole account export, or a plain .fit). They go to the server one at a time with a
 * progress bar; the server reads every FIT file in them as this account's own data and answers with
 * what it found.
 */
export function GarminImportPage() {
  const queue = useImportQueue();
  const [over, setOver] = useState(false);

  function pick(e: ChangeEvent<HTMLInputElement>) {
    const files = Array.from(e.target.files ?? []);
    // Clear the input so picking the same file again fires a change.
    e.target.value = '';
    queue.add(files);
  }

  function drop(e: DragEvent<HTMLLabelElement>) {
    e.preventDefault();
    setOver(false);
    queue.add(Array.from(e.dataTransfer.files));
  }

  const finished = queue.items.some((i) => i.status === 'done' || i.status === 'failed' || i.status === 'cancelled');

  return (
    <SubPage title="Import from Garmin" backTo="/profile">
      <p className="subpage__lede">Add the history you exported from Garmin to your own data. Your server reads the files. Nothing goes anywhere else.</p>
      <Explanation />
      <label
        className={over ? 'dropzone dropzone--over' : 'dropzone'}
        onDragOver={(e) => {
          e.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={drop}
      >
        <ImportIcon size={28} />
        <span className="dropzone__title">Drop files here</span>
        <span className="dropzone__hint">or tap to choose .zip and .fit files</span>
        <input type="file" accept=".zip,.fit" multiple className="sr-only" aria-label="Garmin export files" onChange={pick} />
      </label>
      {queue.items.length > 0 ? (
        <Card padding="lg" aria-label="Files to import">
          <CardHeader title="Files" aside={finished ? <Button onClick={queue.clearFinished}>Clear finished</Button> : undefined} />
          <ul className="import-list">
            {queue.items.map((item) => (
              <QueueItem key={item.id} item={item} accepts={isImportable} result={(r) => <Result r={r} />} onCancel={() => queue.cancel(item.id)} onRetry={() => queue.retry(item.id)} />
            ))}
          </ul>
        </Card>
      ) : null}
    </SubPage>
  );
}
