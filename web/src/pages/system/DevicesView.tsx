import type { ReactNode } from 'react';
import type { UseQueryResult } from '@tanstack/react-query';
import type { DevicesResponse, StorageResponse } from '../../api/generated';
import { Button, Card, CardHeader, DataList, ErrorState, LinkButton, Skeleton, Table, useSync } from '../../components';
import { fmtInt, isMissing } from '../../lib/format';
import { useNativeStatus } from '../../lib/hooks';
import { fmtBytes, fmtDateTime } from '../today/format';
import { errorMessage } from './errors';

function Pending<T>({ query, what, height, children }: { query: UseQueryResult<T>; what: string; height: number; children: (data: T) => ReactNode }) {
  if (query.isPending) return <Skeleton height={height} />;
  if (query.data === undefined) {
    return (
      <ErrorState title={`Could not load ${what}`} action={<Button onClick={() => void query.refetch()}>Try again</Button>}>
        {errorMessage(query.error)}
      </ErrorState>
    );
  }
  return <>{children(query.data)}</>;
}

/** "Battery 74%" with the time the watch reported it. */
export function batteryLine(w: DevicesResponse['watch'], timezone: string): string | null {
  if (isMissing(w.battery_percent)) return null;
  const at = fmtDateTime(w.battery_at, timezone);
  return `${fmtInt(w.battery_percent)}%${at ? ` at ${at}` : ''}`;
}

const LINK_LINE = {
  connected: 'Connected over Bluetooth',
  waiting: 'Waiting to sync',
  problem: 'Connection problem',
  unknown: null,
} as const;

export function WatchCard({ query, timezone }: { query: UseQueryResult<DevicesResponse>; timezone: string }) {
  const { sync, spinning, native } = useSync();
  const { link } = useNativeStatus();
  return (
    <Card padding="lg" aria-labelledby="watch-title">
      <CardHeader title="Your watch" id="watch-title" />
      <Pending query={query} what="the watch status" height={140}>
        {(d) => {
          const w = d.watch;
          const battery = isMissing(w.battery_percent) ? null : `${fmtInt(w.battery_percent)}%`;
          const linkText = LINK_LINE[link];
          return (
            <>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 12 }}>
                <div className="stat" style={{ gap: 3 }}>
                  <span style={{ fontSize: 17, fontWeight: 600 }}>{w.model ?? 'Garmin watch'}</span>
                  {linkText ? <span className="stat__sub tone-link">{linkText}</span> : null}
                </div>
                <span className="stat__value" style={{ fontSize: 22 }} aria-label={battery ? `Battery ${battery}` : 'Battery unavailable'}>
                  {battery ?? <span className="unavailable">unavailable</span>}
                </span>
              </div>
              <DataList
                items={[
                  { label: 'Model', value: w.model },
                  { label: 'Firmware', value: w.firmware },
                  { label: 'Battery', value: batteryLine(w, timezone) },
                  { label: 'Last sync', value: fmtDateTime(d.last_sync_at, timezone) },
                  { label: 'Latest watch data', value: fmtDateTime(w.last_data_at, timezone) },
                  { label: 'New files, last 24 hours', value: fmtInt(d.new_files_24h) },
                ]}
              />
              <Button size="lg" variant="primary" onClick={sync} disabled={spinning} aria-busy={spinning}>
                {native ? (spinning ? 'Syncing' : 'Sync now') : spinning ? 'Refreshing' : 'Refresh from the server'}
              </Button>
              {native ? null : <p className="stat__sub">In a browser this reloads what the server has. The phone app talks to the watch.</p>}
            </>
          );
        }}
      </Pending>
    </Card>
  );
}

export function ServerCard({ storage, timezone }: { storage: UseQueryResult<StorageResponse>; timezone: string }) {
  return (
    <Card padding="lg" aria-labelledby="server-title">
      <CardHeader title="Your server" id="server-title" />
      <Pending query={storage} what="storage" height={140}>
        {(s) => (
          <>
            <DataList
              items={[
                { label: 'Address', value: window.location.host },
                { label: 'Status', value: 'Reachable', tone: 'link' },
                { label: 'Original files', value: `${fmtInt(s.files)} files, ${fmtBytes(s.bytes)}` },
                { label: 'Database', value: fmtBytes(s.database_bytes) },
                { label: 'Last backup', value: fmtDateTime(s.last_backup_at, timezone) },
              ]}
            />
            <DataList
              items={[
                { label: 'Read without trouble', value: fmtInt(s.parsed_ok) },
                { label: 'Read in part', value: fmtInt(s.parsed_partial), ...(s.parsed_partial > 0 ? { tone: 'warn' as const } : {}) },
                { label: 'Could not be read', value: fmtInt(s.parsed_error), ...(s.parsed_error > 0 ? { tone: 'warn' as const } : {}) },
              ]}
            />
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
            <p className="stat__sub">Original files are kept as the watch sent them, so the data can be read again later.</p>
          </>
        )}
      </Pending>
    </Card>
  );
}

export function UploadersCard({ query, timezone }: { query: UseQueryResult<DevicesResponse>; timezone: string }) {
  return (
    <Card padding="lg" aria-labelledby="uploaders-title">
      <CardHeader title="Phones and tools that upload" id="uploaders-title" />
      <Pending query={query} what="the upload devices" height={90}>
        {(d) =>
          d.upload_devices.length === 0 ? (
            <p className="stat__sub">No phone has signed in to upload yet. Sign in to the Nimrag Connect app on your phone.</p>
          ) : (
            <Table
              caption="Upload devices"
              wide
              rows={d.upload_devices}
              rowKey={(r) => String(r.id)}
              columns={[
                { key: 'name', header: 'Name', cell: (r) => r.name },
                { key: 'used', header: 'Last upload', cell: (r) => fmtDateTime(r.last_used_at, timezone) ?? 'Never' },
                { key: 'status', header: 'Status', cell: (r) => (r.active ? 'Active' : 'Revoked') },
              ]}
            />
          )
        }
      </Pending>
      <LinkButton to="/settings" size="lg">
        Manage signed-in phones
      </LinkButton>
    </Card>
  );
}
