import { useState, type ChangeEvent, type DragEvent } from 'react';
import type { NimragImportResponse } from '../../api/generated';
import { Button, Card, CardHeader, ImportIcon } from '../../components';
import { SubPage } from './common';
import { QueueItem } from './GarminImportPage';
import { isRestorable, summarizeRestore, useRestoreQueue } from './nimragRestore';

function Explanation() {
  return (
    <Card padding="lg" aria-labelledby="restore-what">
      <CardHeader title="What to pick" id="restore-what" />
      <div className="form">
        <p className="card__intro">
          The zip you got from <strong>Download my data</strong>, from this server or another one. Pick it as it is. Do not unzip it.
        </p>
        <p className="card__intro">
          Your files and entries come back next to what you already have. Nothing is deleted, and picking the same zip again changes nothing. Workouts come back as not
          sent to the watch. Your settings are replaced by the ones in the zip.
        </p>
      </div>
    </Card>
  );
}

function Result({ r }: { r: NimragImportResponse }) {
  const s = summarizeRestore(r);
  return (
    <div className="import-item__result">
      <p className={s.warn ? 'import-item__headline import-item__headline--warn' : 'import-item__headline'} role="status">
        {s.headline}
      </p>
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
        </details>
      ) : null}
    </div>
  );
}

/**
 * Profile, Restore from a Nimrag export: pick or drop the zip from "Download my data". It goes to
 * the server with a progress bar; the server puts the files and the entries in it into this
 * account and answers with what it added.
 */
export function NimragRestorePage() {
  const queue = useRestoreQueue();
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
    <SubPage title="Restore from a Nimrag export" backTo="/profile">
      <p className="subpage__lede">Put the data from a Nimrag export back into your account. Your server reads the zip. Nothing goes anywhere else.</p>
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
        <span className="dropzone__title">Drop the zip here</span>
        <span className="dropzone__hint">or tap to choose a .zip file</span>
        <input type="file" accept=".zip" className="sr-only" aria-label="Nimrag export file" onChange={pick} />
      </label>
      {queue.items.length > 0 ? (
        <Card padding="lg" aria-label="Files to restore">
          <CardHeader title="Files" aside={finished ? <Button onClick={queue.clearFinished}>Clear finished</Button> : undefined} />
          <ul className="import-list">
            {queue.items.map((item) => (
              <QueueItem
                key={item.id}
                item={item}
                accepts={isRestorable}
                result={(r) => <Result r={r} />}
                onCancel={() => queue.cancel(item.id)}
                onRetry={() => queue.retry(item.id)}
              />
            ))}
          </ul>
        </Card>
      ) : null}
    </SubPage>
  );
}
