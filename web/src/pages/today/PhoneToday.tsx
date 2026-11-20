import type { UseQueryResult } from '@tanstack/react-query';
import type { SettingsResponse, Today } from '../../api/generated';
import { Button, Card, ErrorState, SampleDataChip, Skeleton, TopBar } from '../../components';
import { EditHome } from './EditHome';
import { FocusCarousel } from './FocusCarousel';
import { fmtLongDay, syncLine } from './format';
import { AtAGlance, Last7DaysCard, TimezoneNotice, YesterdayCard } from './Sections';

export function TodayLoading() {
  return (
    <div className="page" aria-busy="true" aria-label="Loading today">
      <Card padding="lg">
        <Skeleton height={104} />
        <Skeleton height={84} />
      </Card>
      <div className="glance-grid">
        {Array.from({ length: 4 }, (_, i) => (
          <Card key={i} padding="tile">
            <Skeleton height={14} width="60%" />
            <Skeleton height={26} width="80%" />
            <Skeleton height={14} />
          </Card>
        ))}
      </div>
    </div>
  );
}

export function TodayFailed({ query }: { query: UseQueryResult<Today> }) {
  return (
    <div className="page">
      <ErrorState
        title="Could not load today"
        action={
          <Button variant="primary" onClick={() => void query.refetch()}>
            Try again
          </Button>
        }
      >
        {query.error instanceof Error ? query.error.message : 'The server did not answer.'}
      </ErrorState>
    </div>
  );
}

interface Props {
  today: UseQueryResult<Today>;
  settings: UseQueryResult<SettingsResponse>;
}

/** Today on a phone (Main.dc.html plus the owner's structure): In focus, At a glance, Yesterday, Last 7 days, Edit home. */
export function PhoneToday({ today, settings }: Props) {
  const t = today.data;
  return (
    <>
      <TopBar
        variant="large"
        subtitle={t ? fmtLongDay(t.date) : null}
        title="Today"
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
          <TimezoneNotice timezone={t.timezone} />
          <FocusCarousel today={t} />
          <AtAGlance today={t} settings={settings.data} />
          {settings.isError ? <p className="today-note">Your saved card layout could not be loaded, so the default one is shown.</p> : null}
          <YesterdayCard today={t} />
          <Last7DaysCard today={t} />
          <div className="today-actions">
            <EditHome settings={settings.data} block />
          </div>
        </div>
      ) : (
        <TodayFailed query={today} />
      )}
    </>
  );
}
