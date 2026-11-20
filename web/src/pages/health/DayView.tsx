import { ProgressBar, StatTile, TopBar, ListRow } from '../../components';
import type { DayResponse } from '../../api/generated/DayResponse';
import { addDays, browserToday, fmtShortDay } from '../../lib/dates';
import { fmtInt, withUnit } from '../../lib/format';
import { usePageTitle } from '../../lib/hooks';
import { ActivitiesSection, EnergyCard, HeartRateCard, StressCard } from './dayCards';
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
