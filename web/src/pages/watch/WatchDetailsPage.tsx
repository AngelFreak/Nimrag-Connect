import type { StorageResponse } from '../../api/generated';
import { Button, Card, DataList, ErrorState, Skeleton, Table } from '../../components';
import { fmtInt } from '../../lib/format';
import { SubPage } from '../profile/common';
import { errorMessage } from '../system/errors';
import { fmtBytes, fmtDateTime } from '../today/format';
import { useDevices, useSettings, useStorage } from '../today/queries';

/** The parse counters, each only when it has something to say. `null` when all three are zero. */
export function parseItems(s: Pick<StorageResponse, 'parsed_ok' | 'parsed_partial' | 'parsed_error'>) {
  const items = [
    { label: 'Read without trouble', count: s.parsed_ok, warn: false },
    { label: 'Read in part', count: s.parsed_partial, warn: true },
    { label: 'Could not be read', count: s.parsed_error, warn: true },
  ]
    .filter((i) => i.count > 0)
    .map((i) => ({ label: i.label, value: fmtInt(i.count), ...(i.warn ? { tone: 'warn' as const } : {}) }));
  return items.length > 0 ? items : null;
}

/** Watch, Details: what the server holds and how well it could read it. The builder's detail, one tap away. */
export function WatchDetailsPage() {
  const devices = useDevices(true);
  const storage = useStorage(true);
  const settings = useSettings();
  const timezone = settings.data?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone;
  const s = storage.data;
  const w = devices.data?.watch;
  const counters = s ? parseItems(s) : null;

  return (
    <SubPage title="Details" backTo="/devices" backLabel="Back to watch">
      <p className="subpage__lede">Original files are kept as the watch sent them, so the data can be read again later.</p>
      {storage.isPending ? (
        <Card padding="lg" aria-label="Loading the details">
          <Skeleton height={20} lines={3} />
        </Card>
      ) : null}
      {storage.isError && !s ? (
        <Card padding="lg">
          <ErrorState title="Could not load the details" action={<Button onClick={() => void storage.refetch()}>Try again</Button>}>
            {errorMessage(storage.error)}
          </ErrorState>
        </Card>
      ) : null}
      {s ? (
        <Card padding="lg" aria-label="Files">
          {counters ? <DataList items={counters} /> : <p className="subpage__lede">No files have been read yet.</p>}
          {s.by_source.length > 0 ? (
            <Table
              caption="Original files by source"
              density="compact"
              rows={s.by_source}
              rowKey={(r) => r.source}
              columns={[
                { key: 'source', header: 'Source', cell: (r) => r.source },
                { key: 'files', header: 'Files', align: 'end', cell: (r) => fmtInt(r.files) },
                { key: 'bytes', header: 'Size', align: 'end', cell: (r) => fmtBytes(r.bytes) },
              ]}
            />
          ) : null}
        </Card>
      ) : null}
      {w ? (
        <Card padding="lg" aria-label="Watch">
          <DataList
            hideMissing
            items={[
              { label: 'Model', value: w.model },
              { label: 'Firmware', value: w.firmware },
              { label: 'Last sync', value: fmtDateTime(devices.data?.last_sync_at, timezone) },
              { label: 'Latest watch data', value: fmtDateTime(w.last_data_at, timezone) },
            ]}
            empty="The watch has not reported anything yet."
          />
        </Card>
      ) : null}
    </SubPage>
  );
}
