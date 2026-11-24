import type { ReactNode } from 'react';
import { Card, CardHeader, EmptyState, Legend, ListRow, SeriesChart, StressBars, Unavailable, type XLabel } from '../../components';
import type { DayResponse } from '../../api/generated/DayResponse';
import { fmtClock, fmtInt } from '../../lib/format';
import { LEVEL_COLOR, LEVEL_NAME, LEVEL_RANGE, daySentence, levelSeconds, type MeasuredLevel } from '../../lib/stress';
import { useIsDesktop } from '../../lib/hooks';
import { energyAside, energySummary, fmtSeconds, hourSeries, hrAside, hrSummary, kindName, stressAside, stressBuckets, stressSummary } from './model';
import { FactorList, FactorRow, StatsSection } from './desktop';
import { ChartSummary } from './parts';
import { clock } from './time';

// The cards of one day (heart rate, stress, energy, activities), shared by the Day view and the 1-day
// tab of the metric pages.

export const HOURS = ['00', '06', '12', '18', '24'];
/** The hour labels as the chart's x axis: "00" at hour 0 ... "24" at hour 24. */
export const HOUR_LABELS: XLabel[] = HOURS.map((text, i) => ({ value: i * 6, text }));

/** A phone shows a card with a small title; a wide screen a titled section on the sheet, no card. */
export function Panel({ title, aside, children, padding, muted }: { title: string; aside?: ReactNode; children: ReactNode; padding?: 'lg'; muted?: boolean }) {
  const desktop = useIsDesktop();
  if (desktop) {
    return (
      <StatsSection title={title} aside={aside}>
        {children}
      </StatsSection>
    );
  }
  return (
    <Card {...(padding ? { padding } : {})}>
      <CardHeader title={title} aside={aside} {...(muted ? { tone: 'muted' as const } : {})} />
      {children}
    </Card>
  );
}

export function HeartRateCard({ day }: { day: DayResponse }) {
  const desktop = useIsDesktop();
  const hr = hourSeries(day.date, day.heart_rate.day.series);
  return (
    <Panel title="Heart rate" aside={hrAside(day.heart_rate) ?? <Unavailable />}>
      <SeriesChart
        label="Heart rate across the day"
        summary={hrSummary(day.heart_rate)}
        x={hr?.x ?? []}
        xRange={[0, 24]}
        series={[{ label: 'Heart rate', color: 'heart', data: hr?.y ?? [] }]}
        height={desktop ? 200 : 110}
        xLabels={HOUR_LABELS}
      />
    </Panel>
  );
}

/**
 * The day's stress: bars coloured by level, the colour key, and a plain line about the day. The key
 * shows each level's time; with `ranges` it shows the value range of each level instead (the metric
 * page has the times in its own bar).
 */
export function StressCard({ day, ranges = false }: { day: DayResponse; ranges?: boolean }) {
  const st = day.stress;
  const buckets = stressBuckets(day.date, st.series);
  const secs = levelSeconds(st);
  const item = (level: MeasuredLevel) => ({
    key: level,
    label: ranges ? (
      <>
        {LEVEL_NAME[level]} {LEVEL_RANGE[level]}
      </>
    ) : (
      <>
        {LEVEL_NAME[level]} {fmtSeconds(secs[level]) ?? <Unavailable />}
      </>
    ),
    color: LEVEL_COLOR[level],
  });
  return (
    <Panel title="Stress" aside={stressAside(st) ?? <Unavailable />}>
      <StressBars buckets={buckets} label="Stress level across the day" xLabels={HOURS} />
      <Legend
        items={[
          item('rest'),
          item('low'),
          item('medium'),
          ...(ranges || st.high_s ? [item('high')] : []),
          { key: 'activity', label: ranges ? 'Activity' : <>Activity {fmtSeconds(st.activity_s) ?? <Unavailable />}</>, color: LEVEL_COLOR.activity, outlined: true },
        ]}
      />
      <ChartSummary quiet>{daySentence(st) ?? stressSummary(st)}</ChartSummary>
    </Panel>
  );
}

export function EnergyCard({ day }: { day: DayResponse }) {
  const desktop = useIsDesktop();
  const energy = hourSeries(day.date, day.energy.series);
  const wake = clock(day.energy.wake_time);
  return (
    <Panel title="Energy" aside={energyAside(day.energy) ?? <Unavailable />}>
      <SeriesChart
        label="Energy across the day"
        summary={`${energySummary(day.energy)}${wake ? ` Woke at ${wake}.` : ''}`}
        x={energy?.x ?? []}
        xRange={[0, 24]}
        series={[{ label: 'Energy', color: 'energy', data: energy?.y ?? [], area: true }]}
        yRange={[0, 100]}
        height={desktop ? 180 : 100}
        xLabels={HOUR_LABELS}
      />
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13 }}>
        <span className="tone-energy">{day.energy.charged === null ? <Unavailable /> : `+${fmtInt(day.energy.charged)} charged`}</span>
        <span className="tone-warn">{day.energy.drained === null ? <Unavailable /> : `-${fmtInt(day.energy.drained)} drained`}</span>
      </div>
    </Panel>
  );
}

export function ActivitiesSection({ day }: { day: DayResponse }) {
  const desktop = useIsDesktop();
  if (desktop) {
    return (
      <StatsSection title="Activities">
        {day.activities.length === 0 ? (
          <p className="stat__sub">No activities this day.</p>
        ) : (
          <FactorList tight label="Activities this day">
            {day.activities.map((a) => (
              <FactorRow
                key={a.id}
                to={`/activities/${a.id}`}
                name={a.sport_name && a.kind === 'other' ? a.sport_name : kindName(a.kind)}
                sub={[clock(a.start), a.distance_m === null ? null : `${(a.distance_m / 1000).toFixed(2)} km`, a.avg_hr_bpm === null ? null : `${fmtInt(a.avg_hr_bpm)} bpm avg`].filter(Boolean).join(' · ')}
                rating={fmtClock(a.duration_s) ?? undefined}
              />
            ))}
          </FactorList>
        )}
      </StatsSection>
    );
  }
  return (
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
              sub={[clock(a.start), a.distance_m === null ? null : `${(a.distance_m / 1000).toFixed(2)} km`, a.avg_hr_bpm === null ? null : `${fmtInt(a.avg_hr_bpm)} bpm avg`]
                .filter(Boolean)
                .join(' · ')}
              value={fmtClock(a.duration_s)}
            />
          ))}
        </div>
      )}
    </section>
  );
}
