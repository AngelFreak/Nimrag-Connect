import type { ReactNode } from 'react';
import type { UseQueryResult } from '@tanstack/react-query';
import type { DevicesResponse } from '../api/generated';
import { Button, Card, Chip, DetailsIcon, ErrorState, FilesIcon, PhoneIcon, SampleDataChip, ServerIcon, SettingsGroup, SettingsRow, Skeleton, StatusBanner, TopBar, Unavailable, useSync, WatchIcon } from '../components';
import { fmtInt, isMissing } from '../lib/format';
import { useNativeStatus, usePageTitle } from '../lib/hooks';
import type { LinkState } from '../native';
import { phonesSummary } from './profile/common';
import { errorMessage } from './system/errors';
import { useTokens } from './system/queries';
import { fmtBytes } from './today/format';
import { useDevices, useStorage } from './today/queries';
import { fmtAgo, isStale } from './watch/format';

// GET /api/devices and GET /api/storage (what the server knows) plus the app bridge (what the phone
// knows: the link to the watch, and the Sync now button). Inside the Android app the Watch tab is native;
// this is the same page for a browser, and the same structure: a status card, then rows with values.

/** The one status chip: the phone's own word when the app tells it, otherwise how fresh the server's data is (the last upload, or the newest watch data when nothing was uploaded, e.g. an import). */
export function watchStatus(link: LinkState, lastSyncAt: string | null, now?: number): string {
  if (link === 'connected') return 'Connected';
  if (link === 'waiting') return 'Waiting to sync';
  if (link === 'problem') return 'Connection problem';
  if (!lastSyncAt) return 'No data yet';
  return isStale(lastSyncAt, now) ? 'Not synced lately' : 'Up to date';
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function StatusCard({ query, link }: { query: UseQueryResult<DevicesResponse>; link: LinkState }) {
  const { sync, spinning, native } = useSync();
  if (query.isPending) {
    return (
      <Card padding="lg" aria-label="Loading the watch status">
        <Skeleton height={140} />
      </Card>
    );
  }
  const d = query.data;
  if (d === undefined) {
    return (
      <Card padding="lg">
        <ErrorState title="Could not load the watch status" action={<Button onClick={() => void query.refetch()}>Try again</Button>}>
          {errorMessage(query.error)}
        </ErrorState>
      </Card>
    );
  }
  const w = d.watch;
  const battery = isMissing(w.battery_percent) ? null : `${fmtInt(w.battery_percent)}%`;
  const synced = fmtAgo(d.last_sync_at);
  const latest = fmtAgo(w.last_data_at);
  return (
    <>
      <section className="watch-card" aria-label="Watch status">
        <div className="watch-card__head">
          <span className="srow__icon" data-tone="accent" aria-hidden="true">
            <WatchIcon />
          </span>
          <div className="watch-card__text">
            <h2 className="watch-card__name">{w.model ?? 'Garmin watch'}</h2>
            <Chip>{watchStatus(link, d.last_sync_at ?? w.last_data_at)}</Chip>
          </div>
        </div>
        <dl className="watch-card__facts">
          <Fact label="Battery">{battery ?? <Unavailable />}</Fact>
          <Fact label="Synced">{synced ?? 'Never'}</Fact>
          <Fact label="Latest data">{latest ?? <Unavailable />}</Fact>
          <Fact label="New files, 24 h">{fmtInt(d.new_files_24h)}</Fact>
        </dl>
        {native ? (
          <Button size="lg" variant="primary" onClick={sync} disabled={spinning} aria-busy={spinning}>
            {spinning ? 'Syncing' : 'Sync now'}
          </Button>
        ) : null}
      </section>
      {native && link === 'problem' ? (
        <StatusBanner action={<Button onClick={sync} disabled={spinning}>Try again</Button>}>The phone could not reach your watch.</StatusBanner>
      ) : null}
    </>
  );
}

/** Watch: status first, then one row per place the watch's data passes through. */
export function WatchPage() {
  usePageTitle('Watch');
  const devices = useDevices(true);
  const storage = useStorage(true);
  const tokens = useTokens();
  const { link } = useNativeStatus();
  const s = storage.data;
  const sample = devices.data?.sample ?? s?.sample;
  const problems = (s?.parsed_partial ?? 0) + (s?.parsed_error ?? 0);

  return (
    <>
      <TopBar variant="back" backTo="/" title="Watch" subtitle="Your watch, server and phones" aside={<SampleDataChip sample={sample} />} />
      <div className="page" aria-busy={devices.isPending || storage.isPending}>
        <div className="watch-page">
          <StatusCard query={devices} link={link} />

          <SettingsGroup title="Server">
            <SettingsRow icon={<ServerIcon />} tone="energy" title="Address" value={window.location.host} info />
            <SettingsRow icon={<ServerIcon />} tone="energy" title="Status" value={s ? 'Reachable' : storage.isError ? 'Not answering' : undefined} info />
            {s && s.database_bytes !== null ? <SettingsRow icon={<FilesIcon />} tone="muted" title="Database" value={fmtBytes(s.database_bytes)} info /> : null}
            {s ? <SettingsRow icon={<FilesIcon />} tone="muted" title="Original files" value={`${fmtInt(s.files)} files, ${fmtBytes(s.bytes)}`} info /> : null}
          </SettingsGroup>
          {storage.isError && !s ? (
            <Card padding="lg">
              <ErrorState title="Could not load the server details" action={<Button onClick={() => void storage.refetch()}>Try again</Button>}>
                {errorMessage(storage.error)}
              </ErrorState>
            </Card>
          ) : null}

          <div className="watch-page__side">
            <SettingsGroup title="Sync">
              <SettingsRow icon={<PhoneIcon />} tone="sleep" title="Phones" value={tokens.data ? phonesSummary(tokens.data.tokens) : tokens.isError ? 'Could not load' : undefined} to="/profile/phones" />
            </SettingsGroup>

            <SettingsGroup title="More">
              <SettingsRow
                icon={<DetailsIcon />}
                tone="muted"
                title="Details"
                value={s ? (problems > 0 ? `${fmtInt(problems)} ${problems === 1 ? 'file' : 'files'} not fully read` : 'All files read') : undefined}
                to="/devices/details"
              />
            </SettingsGroup>
          </div>
        </div>
      </div>
    </>
  );
}
