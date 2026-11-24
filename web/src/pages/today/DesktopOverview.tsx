import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import type { UseQueryResult } from '@tanstack/react-query';
import type { SettingsResponse, Today } from '../../api/generated';
import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import { Button, DataList, LinkButton, ProgressBar, SampleDataChip, Skeleton } from '../../components';
import { fmtShortDay } from '../../lib/dates';
import { fmtFixed, fmtInt, isMissing } from '../../lib/format';
import { useWorkoutList } from '../workouts/api';
import { DEFAULT_HOME_CARDS, DESKTOP_FOCUS_CARDS, visibleCards } from './cards';
import { EditHome } from './EditHome';
import { activityTitle, fmtBytes, fmtDateTime, fmtKm, fmtSeconds, kindLabel, signed, syncLine, unit } from './format';
import { Glyph, glyphForKind, type GlyphName } from './glyphs';
import { FocusCards, HomeGlanceCard, type ActivitiesState } from './homeCards';
import { TodayFailed } from './PhoneToday';
import { useDevices, useRecentActivities, useSleepNights, useStorage } from './queries';
import { TimezoneNotice } from './Sections';

/** Colour of a sport's round icon in the Recent activities list. */
const SPORT_COLOR: Record<string, string> = {
  run: '#f27a1a',
  cycle: '#16a34a',
  walk: '#0f9d8f',
  hike: '#0f9d8f',
  swim: '#1265c2',
  strength: '#7b5cff',
  training: '#7b5cff',
  other: '#e23b3b',
};

function join(...parts: (string | null | undefined | false)[]): string | null {
  const present = parts.filter((p): p is string => typeof p === 'string' && p.length > 0);
  return present.length > 0 ? present.join(' · ') : null;
}

// ---------------------------------------------------------------- Yesterday and Last 7 Days

interface RowProps {
  glyph: GlyphName;
  tone: string;
  label: string;
  to: string;
  value: ReactNode;
  aside?: ReactNode;
}

/** One 52 px row: icon and blue link, the value, and optionally something at the right (the steps goal bar). */
function SummaryRow({ glyph, tone, label, to, value, aside }: RowProps) {
  return (
    <li className="hrows__row">
      <span className="hrows__name">
        <span className="hrows__icon" style={{ color: tone }}>
          <Glyph name={glyph} />
        </span>
        <Link to={to}>{label}</Link>
      </span>
      <span className="hrows__value">{value}</span>
      {aside ? <span className="hrows__aside">{aside}</span> : null}
    </li>
  );
}

function present(value: ReactNode): boolean {
  return !isMissing(value) && value !== '' && value !== false;
}

function YesterdayRows({ today, stepGoal }: { today: Today; stepGoal: number }) {
  const y = today.yesterday;
  const flows =
    isMissing(y.energy_charged) && isMissing(y.energy_drained)
      ? null
      : `${isMissing(y.energy_charged) ? 'unavailable' : signed(y.energy_charged)} / ${isMissing(y.energy_drained) ? 'unavailable' : signed(-y.energy_drained)}`;
  const pct = !isMissing(y.steps) && stepGoal > 0 ? Math.round((y.steps / stepGoal) * 100) : null;
  const rows: (RowProps & { key: string })[] = [
    { key: 'energy', glyph: 'energy', tone: 'var(--energy)', label: 'Energy', to: '/metrics/energy', value: flows },
    {
      key: 'steps',
      glyph: 'steps',
      tone: 'var(--steps)',
      label: 'Steps',
      to: '/metrics/steps',
      value: fmtInt(y.steps),
      aside:
        pct === null ? null : (
          <>
            <span>{`${pct}% of ${fmtInt(stepGoal)}`}</span>
            <span className="hrows__bar">
              <ProgressBar value={y.steps} max={stepGoal} color="var(--good)" height={6} label={`${fmtInt(y.steps) ?? 'unavailable'} of ${fmtInt(stepGoal)} steps`} />
            </span>
          </>
        ),
    },
    { key: 'sleep', glyph: 'sleep', tone: 'var(--sleep)', label: 'Sleep Score', to: '/sleep', value: join(isMissing(y.sleep_score) ? null : fmtInt(y.sleep_score), fmtSeconds(y.sleep_duration_s))?.replace(' · ', ' / ') ?? null },
    {
      key: 'hr',
      glyph: 'heart',
      tone: 'var(--heart)',
      label: 'Heart Rate',
      to: '/metrics/heart_rate',
      value: join(isMissing(y.resting_hr_bpm) ? null : `${fmtInt(y.resting_hr_bpm)} Rest`, isMissing(y.hr_high_bpm) ? null : `${fmtInt(y.hr_high_bpm)} High`)?.replace(' · ', ' / ') ?? null,
    },
    { key: 'stress', glyph: 'stress', tone: 'var(--stress)', label: 'Stress', to: '/metrics/stress', value: fmtInt(y.stress_avg) },
    { key: 'hrv', glyph: 'hrv', tone: 'var(--heart)', label: 'HRV Status', to: '/hrv', value: isMissing(y.hrv_7d_avg_ms) ? null : `${unit(y.hrv_7d_avg_ms, 'ms')} 7d Avg` },
    { key: 'intensity', glyph: 'intensity', tone: 'var(--warn)', label: 'Intensity Minutes', to: '/metrics/intensity_minutes', value: fmtInt(y.intensity_minutes) },
  ];
  const shown = rows.filter((r) => present(r.value));
  return (
    <section className="hsection" aria-labelledby="yesterday-title">
      <h2 className="section-title" id="yesterday-title">
        Yesterday
      </h2>
      {shown.length === 0 ? (
        <p className="today-note">The watch has no data for this day.</p>
      ) : (
        <ul className="hrows">
          {shown.map(({ key, ...r }) => (
            <SummaryRow key={key} {...r} />
          ))}
        </ul>
      )}
    </section>
  );
}

function Last7Rows({ today }: { today: Today }) {
  const w = today.last_7_days;
  const kinds = w.activities_by_kind.map((k) => `${k.count} ${kindLabel(k.kind).toLowerCase()}`).join(', ');
  const rows: (RowProps & { key: string })[] = [
    {
      key: 'activities',
      glyph: 'activities',
      tone: 'var(--zone-1)',
      label: 'Activities',
      to: '/activities',
      value: w.activity_count === 0 ? 'None' : join(String(w.activity_count), kinds),
    },
    { key: 'steps', glyph: 'steps', tone: 'var(--steps)', label: 'Steps', to: '/metrics/steps', value: isMissing(w.avg_steps) ? null : `${fmtInt(w.avg_steps)} Avg` },
    {
      key: 'sleep',
      glyph: 'sleep',
      tone: 'var(--sleep)',
      label: 'Sleep Score',
      to: '/sleep',
      value: join(isMissing(w.avg_sleep_score) ? null : `${fmtInt(w.avg_sleep_score)} Avg`, isMissing(w.avg_sleep_duration_s) ? null : `${fmtSeconds(w.avg_sleep_duration_s)} Avg`)?.replace(' · ', ' / ') ?? null,
    },
    { key: 'hr', glyph: 'heart', tone: 'var(--heart)', label: 'Resting Heart Rate', to: '/metrics/heart_rate', value: isMissing(w.avg_resting_hr_bpm) ? null : `${unit(w.avg_resting_hr_bpm, 'bpm')} Avg` },
    { key: 'stress', glyph: 'stress', tone: 'var(--stress)', label: 'Stress', to: '/metrics/stress', value: isMissing(w.avg_stress) ? null : `${fmtFixed(w.avg_stress, 0)} Avg` },
  ];
  const shown = rows.filter((r) => present(r.value));
  return (
    <section className="hsection" aria-labelledby="last7-title">
      <h2 className="section-title" id="last7-title">
        Last 7 Days
      </h2>
      {shown.length === 0 ? (
        <p className="today-note">No data for these days.</p>
      ) : (
        <ul className="hrows hrows--two">
          {shown.map(({ key, ...r }) => (
            <SummaryRow key={key} {...r} />
          ))}
        </ul>
      )}
    </section>
  );
}

// ---------------------------------------------------------------- left column

function InFocus({ today, settings, activities, status }: { today: Today; settings: SettingsResponse | undefined; activities: ActivitiesState; status: ReactNode }) {
  const nightsQuery = useSleepNights(14, true);
  const cards = settings?.home_cards ?? DEFAULT_HOME_CARDS;
  return (
    <section className="hsection" aria-labelledby="focus-title">
      <div className="hsection__head">
        <h2 className="section-title" id="focus-title">
          In Focus
        </h2>
        {status}
      </div>
      <div className="hgrid hgrid--focus">
        <FocusCards today={today} hidden={cards.hidden} activities={activities} nights={nightsQuery.data?.nights} />
      </div>
    </section>
  );
}

function AtAGlance({ today, settings }: { today: Today; settings: SettingsResponse | undefined }) {
  const ids = visibleCards(settings?.home_cards ?? DEFAULT_HOME_CARDS).filter((id) => !DESKTOP_FOCUS_CARDS.includes(id));
  return (
    <section className="hsection" aria-labelledby="glance-title">
      <div className="hsection__head">
        <h2 className="section-title" id="glance-title">
          At a Glance
        </h2>
        <Link className="hsection__link" to="/health">
          See All
        </Link>
      </div>
      {ids.length === 0 ? (
        <p className="glance-empty">Every card is hidden. Use Edit Home below to bring some back.</p>
      ) : (
        <div className="hgrid hgrid--glance">
          {ids.map((id) => (
            <HomeGlanceCard key={id} id={id} today={today} />
          ))}
        </div>
      )}
    </section>
  );
}

// ---------------------------------------------------------------- right column

function RecentActivityItem({ a }: { a: ActivityListItem }) {
  const strength = a.kind === 'strength';
  const first = strength && !isMissing(a.sets) ? { value: `${fmtInt(a.sets)}`, label: 'Sets' } : !isMissing(a.distance_m) ? { value: fmtKm(a.distance_m), label: 'Distance' } : !isMissing(a.avg_hr_bpm) ? { value: fmtInt(a.avg_hr_bpm), label: 'Avg HR' } : null;
  return (
    <li className="hact" data-kind={a.kind}>
      <span className="hact__disc" style={{ background: SPORT_COLOR[a.kind] ?? SPORT_COLOR['other']! }}>
        <Glyph name={glyphForKind(a.kind)} size={20} />
      </span>
      <div className="hact__main">
        <Link className="hact__name" to={`/activities/${a.id}`}>
          {activityTitle(a)}
        </Link>
        <span className="hact__date">{fmtShortDay(a.date)}</span>
        <div className="hact__stats">
          {first ? (
            <span className="hact__stat">
              <span className="hact__v">{first.value}</span>
              <span className="hact__l">{first.label}</span>
            </span>
          ) : null}
          <span className="hact__stat">
            <span className="hact__v">{fmtSeconds(a.duration_s) ?? 'unavailable'}</span>
            <span className="hact__l">Time</span>
          </span>
        </div>
      </div>
    </li>
  );
}

function RecentActivities({ activities }: { activities: ActivitiesState }) {
  return (
    <section className="hsection" aria-labelledby="recent-title">
      <div className="hsection__head">
        <h2 className="section-title" id="recent-title">
          Recent activities
        </h2>
        <Link className="hsection__link" to="/activities">
          View all
        </Link>
      </div>
      {activities.status === 'pending' ? (
        <Skeleton height={120} />
      ) : activities.status === 'error' ? (
        <p className="today-note" role="alert">
          Could not load recent activities.
        </p>
      ) : activities.items.length === 0 ? (
        <div className="hcard hcard--plain">
          <p className="today-note">No activities yet. They appear here after the watch syncs.</p>
        </div>
      ) : (
        <ul className="hacts" aria-label="Recent activities">
          {activities.items.slice(0, 5).map((a) => (
            <RecentActivityItem key={a.id} a={a} />
          ))}
        </ul>
      )}
    </section>
  );
}

function SyncCard({ timezone }: { timezone: string }) {
  const devices = useDevices(true);
  const storage = useStorage(true);
  const d = devices.data;
  const s = storage.data;
  const failed = devices.isError || storage.isError;
  return (
    <section className="hsection" aria-labelledby="sync-title">
      <h2 className="section-title" id="sync-title">
        Sync and watch
      </h2>
      <div className="hcard hcard--plain">
        {devices.isPending || storage.isPending ? (
          <Skeleton height={120} />
        ) : failed || !d || !s ? (
          <>
            <p className="today-note" role="alert">
              Could not load the device status.
            </p>
            <Button onClick={() => void Promise.all([devices.refetch(), storage.refetch()])}>Try again</Button>
          </>
        ) : (
          <DataList
            items={[
              { label: 'Server', value: window.location.host },
              { label: 'Watch', value: d.watch.model ? `${d.watch.model}${isMissing(d.watch.battery_percent) ? '' : ` · battery ${fmtInt(d.watch.battery_percent)}%`}` : isMissing(d.watch.battery_percent) ? null : `Battery ${fmtInt(d.watch.battery_percent)}%` },
              { label: 'Last sync', value: fmtDateTime(d.last_sync_at, timezone) },
              { label: 'Latest watch data', value: fmtDateTime(d.watch.last_data_at, timezone) },
              { label: 'New files, 24 hours', value: fmtInt(d.new_files_24h) },
              { label: 'Files stored', value: `${fmtInt(s.files)} original files · ${fmtBytes(s.bytes)}` },
              { label: 'Database', value: fmtBytes(s.database_bytes), optional: true },
              { label: 'Last backup', value: fmtDateTime(s.last_backup_at, timezone), optional: true },
            ]}
          />
        )}
        <LinkButton to="/devices">Open the Watch page</LinkButton>
      </div>
    </section>
  );
}

function WorkoutsCard() {
  const query = useWorkoutList();
  const items = query.data?.workouts.slice(0, 3) ?? [];
  return (
    <section className="hsection" aria-labelledby="workouts-title">
      <h2 className="section-title" id="workouts-title">
        Workouts
      </h2>
      <div className="hcard hcard--plain">
        {query.isPending ? (
          <Skeleton height={48} />
        ) : items.length > 0 ? (
          <ul className="hwork">
            {items.map((w) => (
              <li key={w.id}>
                <Link to={`/workouts/${w.id}`}>{w.name}</Link>
                <span className="hwork__meta">{join(`${w.steps} ${w.steps === 1 ? 'step' : 'steps'}`, fmtSeconds(w.duration_s))}</span>
              </li>
            ))}
          </ul>
        ) : (
          <div className="hempty">
            <span className="hempty__disc">
              <Glyph name="workout" size={22} />
            </span>
            <p className="hempty__text">{query.isError ? 'Could not load your workouts.' : 'No workouts yet. Build one and send it to your watch.'}</p>
          </div>
        )}
        <LinkButton to="/workouts">Open workouts</LinkButton>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------- page

function HomeLoading() {
  return (
    <div className="hgrid-page" aria-busy="true" aria-label="Loading home">
      <div className="hmain">
        <Skeleton height={28} width="30%" />
        <div className="hgrid hgrid--focus">
          {Array.from({ length: 4 }, (_, i) => (
            <div className="hcard hcard--focus" key={i}>
              <Skeleton height={20} width="50%" />
              <Skeleton height={32} width="40%" />
              <Skeleton height={140} />
            </div>
          ))}
        </div>
      </div>
      <div className="hside-col">
        <Skeleton height={28} width="50%" />
        <Skeleton height={200} />
      </div>
    </div>
  );
}

interface Props {
  today: UseQueryResult<Today>;
  settings: UseQueryResult<SettingsResponse>;
}

/** The Home of the web layout (docs/design/web-layout/SPEC.md, part B): In Focus, At a Glance, Yesterday and Last 7 Days; recent activities, sync and workouts at the right. */
export function DesktopOverview({ today, settings }: Props) {
  const t = today.data;
  const recent = useRecentActivities(true);
  const activities: ActivitiesState = recent.data ? { status: 'ready', items: recent.data.items } : recent.isError ? { status: 'error' } : { status: 'pending' };
  return (
    <>
      <h1 className="sr-only">Home</h1>
      {today.isPending ? (
        <HomeLoading />
      ) : t ? (
        <div className="hgrid-page">
          <div className="hmain">
            <TimezoneNotice timezone={t.timezone} />
            <InFocus
              today={t}
              settings={settings.data}
              activities={activities}
              status={
                <div className="hstatus">
                  <SampleDataChip sample={t.sample} />
                  <span>{syncLine(t.last_sync_at, t.last_data_at, t.timezone)}</span>
                </div>
              }
            />
            <AtAGlance today={t} settings={settings.data} />
            {settings.isError ? <p className="today-note">Your saved card layout could not be loaded, so the default one is shown.</p> : null}
            <YesterdayRows today={t} stepGoal={settings.data?.step_goal ?? t.steps.goal} />
            <Last7Rows today={t} />
            <div className="today-actions">
              <EditHome settings={settings.data} desktop />
            </div>
          </div>
          <aside className="hside-col" aria-label="Activities, sync and workouts">
            <RecentActivities activities={activities} />
            <SyncCard timezone={t.timezone} />
            <WorkoutsCard />
          </aside>
        </div>
      ) : (
        <TodayFailed query={today} />
      )}
    </>
  );
}
