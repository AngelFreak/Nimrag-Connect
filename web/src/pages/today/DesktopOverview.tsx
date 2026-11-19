import { useState, type ReactNode } from 'react';
import { Link } from 'react-router-dom';
import type { UseQueryResult } from '@tanstack/react-query';
import type { SettingsResponse, Today } from '../../api/generated';
import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import {
  BarSeries,
  Button,
  Card,
  CardHeader,
  DataList,
  ErrorState,
  LinkButton,
  SampleDataChip,
  SegmentedControl,
  SeriesChart,
  Skeleton,
  StatTile,
  Table,
  TopBar,
} from '../../components';
import { fmtDay, fmtShortDay } from '../../lib/dates';
import { fmtInt, isMissing } from '../../lib/format';
import { EditHome } from './EditHome';
import { activityDetail, activityTitle, fmtBytes, fmtDateTime, fmtLongDay, fmtSeconds, signed, syncLine, unit } from './format';
import { TodayFailed, TodayLoading } from './PhoneToday';
import { useDevices, useRecentActivities, useSleepNights, useStorage, useTrends, type TrendRange } from './queries';
import { AtAGlance, Last7DaysCard, YesterdayCard } from './Sections';

const RANGES = [
  { value: '7d', label: 'Week' },
  { value: '4w', label: 'Month' },
  { value: '3m', label: '3 months' },
  { value: '1y', label: 'Year' },
] as const;

const RANGE_TITLE: Record<TrendRange, string> = {
  '7d': 'last 7 nights',
  '4w': 'last 4 weeks',
  '3m': 'last 3 months',
  '1y': 'last year',
};

/** Shows a skeleton while loading, an error with "Try again", or the content. */
function Body<T>({ query, what, children }: { query: UseQueryResult<T>; what: string; children: (data: T) => ReactNode }) {
  if (query.isPending) return <Skeleton height={120} />;
  if (query.data === undefined) {
    return (
      <ErrorState
        title={`Could not load ${what}`}
        action={
          <Button onClick={() => void query.refetch()}>
            Try again
          </Button>
        }
      >
        {query.error instanceof Error ? query.error.message : undefined}
      </ErrorState>
    );
  }
  return <>{children(query.data)}</>;
}

/** A few evenly spaced date labels for a chart's x axis. */
function dateTicks(dates: string[], count = 5): string[] {
  if (dates.length === 0) return [];
  const n = Math.min(count, dates.length);
  const out: string[] = [];
  for (let i = 0; i < n; i += 1) {
    const idx = n === 1 ? 0 : Math.round((i * (dates.length - 1)) / (n - 1));
    out.push(fmtDay(dates[idx]!, { day: 'numeric', month: 'short' }));
  }
  return out;
}

function HrvPanel() {
  const [range, setRange] = useState<TrendRange>('4w');
  const query = useTrends(range, true);
  return (
    <Card padding="xl" as="section" aria-labelledby="hrv-title">
      <CardHeader title={`HRV, ${RANGE_TITLE[range]}`} id="hrv-title" tone="lg" aside="Shaded band is your usual range" />
      <SegmentedControl label="HRV time range" options={RANGES} value={range} onChange={setRange} />
      <Body query={query} what="the HRV chart">
        {(d) => {
          const points = d.hrv.series.points;
          const values = points.map((p) => p.value);
          const known = values.filter((v): v is number => v !== null);
          const band: [number, number] | undefined =
            !isMissing(d.hrv.baseline_low_ms) && !isMissing(d.hrv.baseline_high_ms) ? [d.hrv.baseline_low_ms, d.hrv.baseline_high_ms] : undefined;
          const summary =
            known.length === 0
              ? 'No overnight HRV values in this period.'
              : `Overnight HRV for ${known.length} nights, average ${fmtInt(d.hrv.series.average)} milliseconds, from ${fmtInt(d.hrv.series.min)} to ${fmtInt(d.hrv.series.max)}${band ? `; your usual range is ${band[0]} to ${band[1]}` : ''}.`;
          return (
            <>
              <SeriesChart
                label={`Overnight HRV, ${RANGE_TITLE[range]}`}
                summary={summary}
                x={values.map((_, i) => i)}
                series={[{ label: 'HRV', color: 'link', data: values }]}
                {...(band ? { band } : {})}
                height={200}
                axes="y"
              />
              <div className="chart__axis" aria-hidden="true">
                {dateTicks(points.map((p) => p.date)).map((t, i) => (
                  <span key={`${t}-${i}`}>{t}</span>
                ))}
              </div>
            </>
          );
        }}
      </Body>
    </Card>
  );
}

function SleepPanel() {
  const query = useSleepNights(14, true);
  return (
    <Card padding="xl" as="section" aria-labelledby="sleep14-title">
      <Body query={query} what="the sleep chart">
        {(d) => {
          const nights = [...d.nights].sort((a, b) => a.date.localeCompare(b.date));
          const hours = nights.map((n) => n.duration_s / 3600);
          const lo = nights.length ? Math.min(...nights.map((n) => n.duration_s)) : null;
          const hi = nights.length ? Math.max(...nights.map((n) => n.duration_s)) : null;
          const label =
            nights.length === 0
              ? 'No sleep recorded in the last 14 nights'
              : `Sleep duration for the last ${nights.length} nights, between ${fmtSeconds(lo)} and ${fmtSeconds(hi)}`;
          return (
            <>
              <CardHeader title="Sleep, last 14 nights" id="sleep14-title" tone="lg" aside={d.average_duration_s === null ? <>Average unavailable</> : `Average ${fmtSeconds(d.average_duration_s)}`} />
              <BarSeries
                values={nights.length ? hours : []}
                labels={nights.map((n) => n.date)}
                axisLabels={dateTicks(nights.map((n) => n.date), 4)}
                label={label}
                highlightIndex={nights.length - 1}
                summary={`${label}.`}
                summaryVisible
                height={200}
              />
            </>
          );
        }}
      </Body>
    </Card>
  );
}

function ActivitiesPanel() {
  const query = useRecentActivities(true);
  return (
    <Card padding="xl" as="section" aria-labelledby="recent-title">
      <CardHeader title="Recent activities" id="recent-title" tone="lg" aside={<Link to="/activities">View all</Link>} />
      <Body query={query} what="recent activities">
        {(d) =>
          d.items.length === 0 ? (
            <p className="today-note">No activities yet. They appear here after the watch syncs.</p>
          ) : (
            <Table<ActivityListItem>
              caption="Recent activities"
              wide
              rows={d.items}
              rowKey={(r) => r.id}
              columns={[
                { key: 'date', header: 'Date', cell: (r) => <Link to={`/activities/${r.id}`}>{fmtShortDay(r.date)}</Link> },
                { key: 'type', header: 'Type', cell: (r) => activityTitle(r) },
                { key: 'dur', header: 'Duration', cell: (r) => fmtSeconds(r.duration_s) },
                { key: 'detail', header: 'Detail', cell: (r) => activityDetail(r), muted: true },
                { key: 'hr', header: 'Avg HR', cell: (r) => (isMissing(r.avg_hr_bpm) ? null : fmtInt(r.avg_hr_bpm)) },
              ]}
            />
          )
        }
      </Body>
    </Card>
  );
}

function SyncPanel({ timezone }: { timezone: string }) {
  const devices = useDevices(true);
  const storage = useStorage(true);
  return (
    <Card padding="xl" as="section" aria-labelledby="sync-title">
      <CardHeader title="Sync and storage" id="sync-title" tone="lg" />
      <p className="today-note">Everything comes from the watch and stays on your server. Original files are kept so data can be re-read later.</p>
      <Body query={devices} what="the device status">
        {(d) => (
          <Body query={storage} what="storage">
            {(s) => (
              <DataList
                layout="wide"
                items={[
                  { label: 'Server', value: window.location.host },
                  { label: 'Watch', value: d.watch.model ? `${d.watch.model}${isMissing(d.watch.battery_percent) ? '' : ` \u00b7 battery ${fmtInt(d.watch.battery_percent)}%`}` : isMissing(d.watch.battery_percent) ? null : `Battery ${fmtInt(d.watch.battery_percent)}%` },
                  { label: 'Last sync', value: fmtDateTime(d.last_sync_at, timezone) },
                  { label: 'Latest watch data', value: fmtDateTime(d.watch.last_data_at, timezone) },
                  { label: 'New files, last 24 hours', value: fmtInt(d.new_files_24h) },
                  { label: 'Files stored', value: `${fmtInt(s.files)} original files \u00b7 ${fmtBytes(s.bytes)}` },
                  { label: 'Database', value: fmtBytes(s.database_bytes) },
                  { label: 'Last backup', value: fmtDateTime(s.last_backup_at, timezone) },
                ]}
              />
            )}
          </Body>
        )}
      </Body>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 12, marginTop: 4 }}>
        <LinkButton to="/devices" variant="primary">
          Devices and sync
        </LinkButton>
      </div>
    </Card>
  );
}

interface Props {
  today: UseQueryResult<Today>;
  settings: UseQueryResult<SettingsResponse>;
}

/** The Overview of the desktop dashboard (Web.dc.html), plus At a glance, Yesterday and Last 7 days. */
export function DesktopOverview({ today, settings }: Props) {
  const t = today.data;
  const tiles = t ? (
    <div className="grid-tiles">
      <StatTile
        size="xl"
        label="Energy now"
        value={fmtInt(t.energy.now)}
        tone="energy"
        sub={isMissing(t.energy.charged) ? undefined : `${signed(t.energy.charged)} charged today`}
      />
      <StatTile
        size="xl"
        label="Sleep last night"
        value={fmtSeconds(t.sleep?.duration_s)}
        tone="sleep"
        sub={t.sleep && !isMissing(t.sleep.score) ? `Score ${fmtInt(t.sleep.score)}` : undefined}
      />
      <StatTile
        size="xl"
        label="Overnight HRV"
        value={unit(t.hrv.last_night_ms, 'ms')}
        sub={!isMissing(t.hrv.baseline_low_ms) && !isMissing(t.hrv.baseline_high_ms) ? `Range ${fmtInt(t.hrv.baseline_low_ms)} to ${fmtInt(t.hrv.baseline_high_ms)}` : undefined}
      />
      <StatTile
        size="xl"
        label="Resting heart rate"
        value={unit(t.heart_rate.resting_hr_bpm, 'bpm')}
        sub={isMissing(t.heart_rate.avg_7d_resting_hr_bpm) ? undefined : `7-day average ${fmtInt(t.heart_rate.avg_7d_resting_hr_bpm)}`}
      />
      <StatTile
        size="xl"
        label="Training this week"
        value={t.training_this_week.sessions === 0 ? 'No sessions' : `${t.training_this_week.sessions} ${t.training_this_week.sessions === 1 ? 'session' : 'sessions'}`}
        sub={t.training_this_week.sessions === 0 ? undefined : `${fmtSeconds(t.training_this_week.duration_s)} total`}
      />
    </div>
  ) : null;

  return (
    <>
      <TopBar
        variant="large"
        subtitle={t ? fmtLongDay(t.date, true) : null}
        title="Overview"
        aside={
          t ? (
            <>
              <SampleDataChip sample={t.sample} />
              <span>{syncLine(t.last_sync_at, t.last_data_at, t.timezone)}</span>
            </>
          ) : null
        }
      />
      {today.isPending ? (
        <TodayLoading />
      ) : t ? (
        <div className="page">
          {tiles}
          <AtAGlance today={t} settings={settings.data} />
          {settings.isError ? <p className="today-note">Your saved card layout could not be loaded, so the default one is shown.</p> : null}
          <div className="grid-panels">
            <HrvPanel />
            <SleepPanel />
          </div>
          <div className="grid-panels">
            <ActivitiesPanel />
            <SyncPanel timezone={t.timezone} />
          </div>
          <div className="grid-panels">
            <YesterdayCard today={t} />
            <Last7DaysCard today={t} />
          </div>
          <div className="today-actions">
            <EditHome settings={settings.data} />
          </div>
        </div>
      ) : (
        <TodayFailed query={today} />
      )}
    </>
  );
}
