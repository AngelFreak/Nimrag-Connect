import { Card, CardHeader, EmptyState, Legend, ListRow, ProgressBar, SeriesChart, StatTile, StressBars, TopBar, Unavailable } from '../../components';
import type { DayResponse } from '../../api/generated/DayResponse';
import { addDays, browserToday, fmtShortDay } from '../../lib/dates';
import { fmtDurationMin, fmtInt, withUnit } from '../../lib/format';
import { useIsDesktop, usePageTitle } from '../../lib/hooks';
import { energyAside, energySummary, fmtSeconds, hourSeries, hrAside, hrSummary, kindName, stepsPercent, stressAside, stressBuckets, stressSummary } from './model';
import { CardSkeleton, ChartSummary, LoadError } from './parts';
import { useDay } from './queries';
import { clock } from './time';

const HOURS = ['00', '06', '12', '18', '24'];
/** The hour labels as the chart's x axis: "00" at hour 0 ... "24" at hour 24. */
const HOUR_LABELS = HOURS.map((text, i) => ({ value: i * 6, text }));

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
  const desktop = useIsDesktop();
  const hr = hourSeries(day.date, day.heart_rate.day.series);
  const energy = hourSeries(day.date, day.energy.series);
  const buckets = stressBuckets(day.date, day.stress.series);
  const st = day.stress;
  const pct = stepsPercent(day.steps, day.step_goal);
  const wake = clock(day.energy.wake_time);
  return (
    <>
      <div className="grid-panels" style={{ alignItems: 'start' }}>
        <Card>
          <CardHeader title="Heart rate" aside={hrAside(day.heart_rate) ?? <Unavailable />} />
          <SeriesChart
            label="Heart rate across the day"
            summary={hrSummary(day.heart_rate)}
            summaryVisible
            x={hr?.x ?? []}
            xRange={[0, 24]}
            series={[{ label: 'Heart rate', color: 'heart', data: hr?.y ?? [] }]}
            height={desktop ? 150 : 110}
            xLabels={HOUR_LABELS}
          />
        </Card>

        <Card>
          <CardHeader title="Stress" aside={stressAside(st) ?? <Unavailable />} />
          <StressBars buckets={buckets} label="Stress level across the day" xLabels={HOURS} />
          <Legend
            items={[
              { key: 'rest', label: <>Rest {fmtSeconds(st.rest_s) ?? <Unavailable />}</>, color: 'var(--stress-rest)' },
              { key: 'low', label: <>Low {fmtSeconds(st.low_s) ?? <Unavailable />}</>, color: 'var(--stress-low)' },
              { key: 'medium', label: <>Medium {fmtSeconds(st.medium_s) ?? <Unavailable />}</>, color: 'var(--stress-medium)' },
              ...(st.high_s ? [{ key: 'high', label: <>High {fmtSeconds(st.high_s)}</>, color: 'var(--heart)' }] : []),
              { key: 'activity', label: <>Activity {fmtSeconds(st.activity_s) ?? <Unavailable />}</>, color: 'var(--stress-activity)', outlined: true },
            ]}
          />
          <ChartSummary>{stressSummary(st)}</ChartSummary>
        </Card>

        <Card>
          <CardHeader title="Energy" aside={energyAside(day.energy) ?? <Unavailable />} />
          <SeriesChart
            label="Energy across the day"
            summary={`${energySummary(day.energy)}${wake ? ` Woke at ${wake}.` : ''}`}
            summaryVisible
            x={energy?.x ?? []}
            xRange={[0, 24]}
            series={[{ label: 'Energy', color: 'energy', data: energy?.y ?? [], area: true }]}
            yRange={[0, 100]}
            height={desktop ? 140 : 100}
            xLabels={HOUR_LABELS}
          />
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13 }}>
            <span className="tone-energy">{day.energy.charged === null ? <Unavailable /> : `+${fmtInt(day.energy.charged)} charged`}</span>
            <span className="tone-warn">{day.energy.drained === null ? <Unavailable /> : `-${fmtInt(day.energy.drained)} drained`}</span>
          </div>
        </Card>

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

      <section aria-labelledby="day-activities" style={{ display: 'flex', flexDirection: 'column', gap: 'var(--gap)' }}>
        <h2 className="section-label" id="day-activities">
          Activities
        </h2>
        {day.activities.length === 0 ? (
          <EmptyState title="No activities this day" />
        ) : (
          <div className={desktop ? 'grid-panels' : undefined} style={desktop ? undefined : { display: 'flex', flexDirection: 'column', gap: 'var(--gap)' }}>
            {day.activities.map((a) => (
              <ListRow
                key={a.id}
                to={`/activities/${a.id}`}
                title={a.sport_name && a.kind === 'other' ? a.sport_name : kindName(a.kind)}
                sub={[clock(a.start), a.distance_m === null ? null : `${(a.distance_m / 1000).toFixed(1)} km`, a.avg_hr_bpm === null ? null : `${fmtInt(a.avg_hr_bpm)} bpm avg`]
                  .filter(Boolean)
                  .join(' · ')}
                value={fmtDurationMin(a.duration_s === null ? null : a.duration_s / 60)}
              />
            ))}
          </div>
        )}
      </section>
    </>
  );
}
