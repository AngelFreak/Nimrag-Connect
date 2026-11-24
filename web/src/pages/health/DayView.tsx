import { useNavigate } from 'react-router-dom';
import { PageHeader, ProgressBar, SampleDataChip, StatStrip, StatTile, TopBar, ListRow } from '../../components';
import type { DayResponse } from '../../api/generated/DayResponse';
import { addDays, browserToday, fmtMonthDay, fmtShortDay } from '../../lib/dates';
import { fmtInt, withUnit } from '../../lib/format';
import { useIsDesktop, usePageTitle } from '../../lib/hooks';
import { ActivitiesSection, EnergyCard, HeartRateCard, StressCard } from './dayCards';
import { DayToolbar, FactorList, FactorRow, ScoreHero, StatsColumns, StatsSection } from './desktop';
import { fmtSeconds, stepsPercent } from './model';
import { CardSkeleton, LoadError } from './parts';
import { useDay } from './queries';
import { clock } from './time';

export function DayView({ date }: { date: string }) {
  const title = fmtShortDay(date);
  usePageTitle(title);
  const q = useDay(date);
  const today = browserToday();
  const prev = q.data?.previous_date ?? addDays(date, -1);
  const next = q.data ? q.data.next_date : date >= today ? null : addDays(date, 1);
  const desktop = useIsDesktop();
  const navigate = useNavigate();
  if (desktop) {
    return (
      <>
        <PageHeader title="Day" actions={<SampleDataChip sample={q.data?.sample} />}>
          <DayToolbar
            label={fmtMonthDay(date)}
            prevLabel="Previous day"
            nextLabel="Next day"
            onPrev={() => void navigate(`/day/${prev}`)}
            onNext={() => next !== null && void navigate(`/day/${next}`)}
            atEnd={next === null}
          />
        </PageHeader>
        <div className="page" style={{ paddingTop: 0 }} aria-busy={q.isPending}>
          {q.isPending ? (
            <div className="stats-cols">
              <CardSkeleton lines={4} height={20} />
              <CardSkeleton lines={3} height={20} />
            </div>
          ) : q.isError ? (
            <LoadError what="this day" error={q.error} onRetry={() => void q.refetch()} />
          ) : (
            <DayDesktop day={q.data} />
          )}
        </div>
      </>
    );
  }
  return (
    <>
      <TopBar
        variant="pager"
        title={title}
        subtitle={q.data?.sample ? 'Day view · sample data' : 'Day view'}
        prevLabel="Previous day"
        nextLabel="Next day"
        prev={{ to: `/day/${prev}` }}
        next={next === null ? null : { to: `/day/${next}` }}
      />
      <div className="page" aria-busy={q.isPending}>
        {q.isPending ? (
          <>
            <CardSkeleton lines={4} height={20} />
            <CardSkeleton lines={3} height={20} />
            <CardSkeleton lines={3} height={20} />
          </>
        ) : q.isError ? (
          <LoadError what="this day" error={q.error} onRetry={() => void q.refetch()} />
        ) : (
          <DayCards day={q.data} />
        )}
      </div>
    </>
  );
}

function DayCards({ day }: { day: DayResponse }) {
  const pct = stepsPercent(day.steps, day.step_goal);
  return (
    <>
      <div className="grid-panels" style={{ alignItems: 'start' }}>
        <HeartRateCard day={day} />
        <StressCard day={day} />
        <EnergyCard day={day} />

        <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--gap)' }}>
          <div className="grid-2">
            <StatTile size="sm" label="Steps" value={fmtInt(day.steps)} sub={pct === null ? undefined : `${pct}% of ${fmtInt(day.step_goal)}`} tone="steps">
              {pct === null ? null : <ProgressBar value={day.steps} max={day.step_goal} label={`${fmtInt(day.steps)} of ${fmtInt(day.step_goal)} steps`} />}
            </StatTile>
            <StatTile size="sm" label="Intensity minutes" value={withUnit(fmtInt(day.intensity_minutes), 'min')} />
          </div>
          <ListRow
            to={`/sleep/${day.date}`}
            title="Sleep"
            sub={day.sleep ? `${clock(day.sleep.start)} to ${clock(day.sleep.end)}` : 'Night of this day'}
            value={day.sleep ? fmtSeconds(day.sleep.duration_s) : null}
            valueSub={day.sleep?.score != null ? `Score ${fmtInt(day.sleep.score)}` : undefined}
          />
        </div>
      </div>

      <ActivitiesSection day={day} />
    </>
  );
}

/** A wide screen: the day's figures in the left column, the charts and activities beside them. */
function DayDesktop({ day }: { day: DayResponse }) {
  const pct = stepsPercent(day.steps, day.step_goal);
  return (
    <StatsColumns
      side={
        <>
          <StatsSection title="Steps">
            <ScoreHero value={fmtInt(day.steps) ?? <span className="unavailable">unavailable</span>} of={day.step_goal ? `of ${fmtInt(day.step_goal)}` : undefined} label={pct === null ? undefined : `${pct}% of the goal`} tone="tone-steps" />
            {pct === null ? null : <ProgressBar value={day.steps} max={day.step_goal} label={`${fmtInt(day.steps)} of ${fmtInt(day.step_goal)} steps`} />}
          </StatsSection>
          <StatStrip items={[{ key: 'intensity', label: 'Intensity minutes', value: withUnit(fmtInt(day.intensity_minutes), 'min') }]} />
          <FactorList label="The night">
            <FactorRow
              to={`/sleep/${day.date}`}
              name="Sleep"
              sub={day.sleep ? `${clock(day.sleep.start)} to ${clock(day.sleep.end)}${day.sleep.score != null ? ` · score ${fmtInt(day.sleep.score)}` : ''}` : 'Night of this day'}
              rating={day.sleep ? (fmtSeconds(day.sleep.duration_s) ?? undefined) : undefined}
            />
          </FactorList>
        </>
      }
    >
      <HeartRateCard day={day} />
      <StressCard day={day} />
      <EnergyCard day={day} />
      <ActivitiesSection day={day} />
    </StatsColumns>
  );
}
