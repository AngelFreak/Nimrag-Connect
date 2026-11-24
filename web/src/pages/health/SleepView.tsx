import { useState } from 'react';
import { Navigate, useNavigate } from 'react-router-dom';
import {
  BarSeries,
  Card,
  CardHeader,
  EmptyState,
  PageHeader,
  SampleDataChip,
  SegmentedControl,
  Skeleton,
  TopBar,
} from '../../components';
import type { NightSeries } from '../../api/generated/NightSeries';
import type { SleepNight } from '../../api/generated/SleepNight';
import type { SleepNightsResponse } from '../../api/generated/SleepNightsResponse';
import { addDays, browserToday, fmtShortDay } from '../../lib/dates';
import { usePageTitle, useIsDesktop } from '../../lib/hooks';
import { axisDates, fmtSeconds, nightBars, nightsSummary } from './model';
import { StatsColumns, StatsSection } from './desktop';
import { CardSkeleton, LoadError } from './parts';
import { sleepFactors } from './sleepNight';
import { FactorsList, NightHeadline, OvernightStrip, ScoreSummary, StagesWithOverlay } from './sleepParts';
import { SleepRangeBar, SleepToolbar } from './SleepRangeView';
import { useSleepNight, useSleepNights, type NightCount } from './queries';

const COUNTS = [
  { value: '7', label: '7 nights' },
  { value: '14', label: '14 nights' },
  { value: '30', label: '30 nights' },
] as const;

/** `/sleep` without a date: go to the latest night the server has. */
export function LatestNight() {
  usePageTitle('Sleep');
  const list = useSleepNights(7);
  const latest = list.data?.nights.at(-1)?.date;
  const desktop = useIsDesktop();
  const navigate = useNavigate();
  if (latest) return <Navigate to={`/sleep/${latest}`} replace />;
  const today = browserToday();
  return (
    <>
      {desktop ? (
        <PageHeader title="Sleep" actions={<SampleDataChip sample={list.data?.sample} />}>
          <SleepToolbar from={today} to={today} atToday onStep={() => {}} unit="night" value="1d" onChange={(k) => void navigate(k === '1d' ? '/sleep' : `/sleep?range=${k}`)} />
        </PageHeader>
      ) : (
        <>
          <TopBar variant="back" backTo="/health" title="Sleep" aside={<SampleDataChip sample={list.data?.sample} />} />
          <SleepRangeBar value="1d" />
        </>
      )}
      <div className="page" style={desktop ? { paddingTop: 0 } : undefined} aria-busy={list.isPending}>
        {list.isPending ? <CardSkeleton lines={3} height={28} /> : null}
        {list.isError ? <LoadError what="sleep" error={list.error} onRetry={() => void list.refetch()} /> : null}
        {list.data ? <EmptyState title="No sleep recorded yet">Nights appear here after the watch has synced.</EmptyState> : null}
      </div>
    </>
  );
}

export function SleepView({ date }: { date: string }) {
  const title = `Sleep · ${fmtShortDay(date)}`;
  usePageTitle(title);
  const [count, setCount] = useState<NightCount>(7);
  const night = useSleepNight(date);
  const list = useSleepNights(count);

  const latest = list.data?.nights.at(-1)?.date ?? null;
  // A night belongs to the day it ends on, so the newest possible one is today's.
  const hasNext = latest !== null ? date < latest : date < browserToday();
  const sample = night.data?.sample ?? list.data?.sample;
  const desktop = useIsDesktop();
  const navigate = useNavigate();

  if (desktop) {
    return (
      <>
        <PageHeader title="Sleep" actions={<SampleDataChip sample={sample} />}>
          <SleepToolbar
            from={date}
            to={date}
            atToday={!hasNext}
            onStep={(d) => void navigate(`/sleep/${addDays(date, d)}`)}
            unit="night"
            value="1d"
            onChange={(k) => void navigate(k === '1d' ? '/sleep' : `/sleep?range=${k}`)}
          />
        </PageHeader>
        <div className="page" style={{ paddingTop: 0 }} aria-busy={night.isPending}>
          {night.isPending ? (
            <div className="stats-cols">
              <CardSkeleton lines={2} height={28} />
              <CardSkeleton lines={4} height={20} />
            </div>
          ) : night.isError ? (
            <LoadError what="this night" error={night.error} onRetry={() => void night.refetch()} />
          ) : (
            <div className="stats-stack">
              {night.data.night === null ? (
                <EmptyState title="No sleep recorded for this night">The watch did not record sleep that night. Try the previous or next night.</EmptyState>
              ) : (
                <NightDesktop night={night.data.night} overlays={night.data.overlays} />
              )}
              <LastNightsDesktop count={count} onCount={setCount} data={list.data} pending={list.isPending} error={list.isError ? list.error : null} onRetry={() => void list.refetch()} />
            </div>
          )}
        </div>
      </>
    );
  }

  return (
    <>
      <TopBar
        variant="back"
        backTo="/health"
        backLabel="Back to Health"
        title="Sleep"
        subtitle={`Night of ${fmtShortDay(addDays(date, -1))}`}
        aside={desktop ? <SampleDataChip sample={sample} /> : undefined}
        prev={{ to: `/sleep/${addDays(date, -1)}` }}
        next={hasNext ? { to: `/sleep/${addDays(date, 1)}` } : null}
        prevLabel="Previous night"
        nextLabel="Next night"
      />
      <SleepRangeBar value="1d" />
      <div className="page" aria-busy={night.isPending}>
        {!desktop && sample ? (
          <div>
            <SampleDataChip sample={sample} />
          </div>
        ) : null}
        <div className="grid-panels" style={{ alignItems: 'start' }}>
          {night.isPending ? (
            <>
              <CardSkeleton lines={2} height={28} />
              <CardSkeleton lines={4} height={20} />
            </>
          ) : night.isError ? (
            <div style={{ gridColumn: '1 / -1' }}>
              <LoadError what="this night" error={night.error} onRetry={() => void night.refetch()} />
            </div>
          ) : night.data.night === null ? (
            <div style={{ gridColumn: '1 / -1' }}>
              <EmptyState title="No sleep recorded for this night">The watch did not record sleep that night. Try the previous or next night.</EmptyState>
            </div>
          ) : (
            <NightCards night={night.data.night} overlays={night.data.overlays} />
          )}
          <LastNights count={count} onCount={setCount} data={list.data} pending={list.isPending} error={list.isError ? list.error : null} onRetry={() => void list.refetch()} />
        </div>
      </div>
    </>
  );
}

function NightCards({ night, overlays }: { night: SleepNight; overlays: NightSeries[] }) {
  const factors = sleepFactors(night);
  return (
    <div className="page-stack" style={{ gridColumn: '1 / -1' }}>
      <Card padding="lg">
        <ScoreSummary night={night} />
        <NightHeadline night={night} factors={factors} />
      </Card>
      <Card padding="lg">
        <CardHeader title="Score factors" tone="muted" />
        <FactorsList factors={factors} />
      </Card>
      <Card padding="lg">
        <CardHeader title="Stages through the night" tone="muted" />
        <StagesWithOverlay night={night} overlays={overlays} />
      </Card>
      <OvernightStrip night={night} />
    </div>
  );
}

function LastNights({
  count,
  onCount,
  data,
  pending,
  error,
  onRetry,
}: {
  count: NightCount;
  onCount: (c: NightCount) => void;
  data: SleepNightsResponse | undefined;
  pending: boolean;
  error: unknown;
  onRetry: () => void;
}) {
  const bars = data ? nightBars(data.nights, count) : null;
  const avg = data ? fmtSeconds(data.average_duration_s) : null;
  const summary = data ? nightsSummary(data.nights, count, data.average_score) : null;
  return (
    <div style={{ gridColumn: '1 / -1' }}>
      <Card padding="lg" className="last-nights">
        <CardHeader title={`Last ${count} nights`} tone="muted" aside={avg ? `Average ${avg}` : undefined} />
        <SegmentedControl label="Number of nights" options={COUNTS} value={String(count) as '7' | '14' | '30'} onChange={(v) => onCount(Number(v) as NightCount)} />
        {pending ? <Skeleton height={84} /> : null}
        {error && !data ? <LoadError what="the last nights" error={error} onRetry={onRetry} /> : null}
        {bars && bars.dates.length > 0 ? (
          <>
            <BarSeries
              values={bars.values}
              labels={bars.labels}
              axisLabels={count === 7 ? undefined : axisDates(bars.dates)}
              label={`Sleep duration for the last ${count} nights`}
              yUnit="minutes"
              color="var(--stage-deep)"
              highlightIndex={bars.values.length - 1}
              summary={summary ?? undefined}
              summaryVisible
              height={count === 7 ? 84 : 96}
            />
          </>
        ) : null}
        {data && bars && bars.dates.length === 0 ? <p className="stat__sub">No nights recorded yet.</p> : null}
      </Card>
    </div>
  );
}

// ---- wide screen ------------------------------------------------------------------------------------

function NightDesktop({ night, overlays }: { night: SleepNight; overlays: NightSeries[] }) {
  const factors = sleepFactors(night);
  return (
    <StatsColumns
      side={
        <>
          <StatsSection title="Sleep Score">
            <ScoreSummary night={night} />
          </StatsSection>
          <NightHeadline night={night} factors={factors} />
        </>
      }
    >
      <StatsSection title="Score factors">
        <FactorsList factors={factors} />
      </StatsSection>

      <StatsSection title="Stages through the night">
        <StagesWithOverlay night={night} overlays={overlays} />
      </StatsSection>

      <StatsSection title="Overnight">
        <OvernightStrip night={night} />
      </StatsSection>
    </StatsColumns>
  );
}

function LastNightsDesktop({
  count,
  onCount,
  data,
  pending,
  error,
  onRetry,
}: {
  count: NightCount;
  onCount: (c: NightCount) => void;
  data: SleepNightsResponse | undefined;
  pending: boolean;
  error: unknown;
  onRetry: () => void;
}) {
  const bars = data ? nightBars(data.nights, count) : null;
  const avg = data ? fmtSeconds(data.average_duration_s) : null;
  const summary = data ? nightsSummary(data.nights, count, data.average_score) : null;
  return (
    <StatsSection title={`Last ${count} nights`} aside={avg ? `Average ${avg}` : undefined}>
      <SegmentedControl label="Number of nights" options={COUNTS} value={String(count) as '7' | '14' | '30'} onChange={(v) => onCount(Number(v) as NightCount)} />
      {pending ? <Skeleton height={84} /> : null}
      {error && !data ? <LoadError what="the last nights" error={error} onRetry={onRetry} /> : null}
      {bars && bars.dates.length > 0 ? (
        <BarSeries
          values={bars.values}
          labels={bars.labels}
          axisLabels={count === 7 ? undefined : axisDates(bars.dates)}
          label={`Sleep duration for the last ${count} nights`}
          yUnit="minutes"
          color="var(--stage-deep)"
          highlightIndex={bars.values.length - 1}
          summary={summary ?? undefined}
          summaryVisible
          height={200}
        />
      ) : null}
      {data && bars && bars.dates.length === 0 ? <p className="stat__sub">No nights recorded yet.</p> : null}
    </StatsSection>
  );
}
