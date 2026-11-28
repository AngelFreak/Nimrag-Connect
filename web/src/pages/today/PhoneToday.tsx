import type { UseQueryResult } from '@tanstack/react-query';
import type { SettingsResponse, Today } from '../../api/generated';
import { Button, Card, ErrorState, SampleDataChip, Skeleton, TopBar } from '../../components';
import { EditHome } from './EditHome';
import { FocusCarousel } from './FocusCarousel';
import { fmtLongDay, syncLine } from './format';
import { AtAGlance, Last7DaysCard, TimezoneNotice, YesterdayCard } from './Sections';

/** Height of the picture of a tile, as in today.css. */
const TILE_VISUAL_HEIGHT = 120;

export function TodayLoading() {
  return (
    <div className="page" aria-busy="true" aria-label="Loading today">
      {/* The same blocks as the loaded page, at about the same heights, so nothing jumps when the data arrives. */}
      <section className="today-section" aria-hidden="true">
        <Card padding="lg">
          <div className="focus-energy">
            <Skeleton height={100} width="100px" />
            <div className="focus-energy__text" style={{ flex: 1 }}>
              <Skeleton height={14} width="40%" />
              <Skeleton height={22} width="80%" />
              <Skeleton height={14} width="60%" />
            </div>
          </div>
          <Skeleton height={142} />
          <Skeleton height={14} width="70%" />
        </Card>
      </section>
      <section className="today-section" aria-hidden="true">
        <Skeleton height={14} width="96px" />
        <div className="glance-grid">
          {Array.from({ length: 6 }, (_, i) => (
            <Card key={i} padding="sm" className="tile">
              <Skeleton height={18} width="60%" />
              <Skeleton height={TILE_VISUAL_HEIGHT} />
              <Skeleton height={28} width="70%" />
              <Skeleton height={14} width="85%" />
            </Card>
          ))}
        </div>
      </section>
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

/** Today on a phone (Main.dc.html plus the owner's structure): In focus (swipeable Energy, Readiness, Sleep, HRV), At a glance tiles, Yesterday, Last 7 days, Edit home. */
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
          <FocusCarousel today={t} {...(settings.data ? { cards: settings.data.home_cards } : {})} />
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
