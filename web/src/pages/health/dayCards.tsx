import { Card, CardHeader, EmptyState, Legend, ListRow, SeriesChart, StressBars, Unavailable, type XLabel } from '../../components';
import type { DayResponse } from '../../api/generated/DayResponse';
import { fmtDurationMin, fmtInt } from '../../lib/format';
import { useIsDesktop } from '../../lib/hooks';
import { energyAside, energySummary, fmtSeconds, hourSeries, hrAside, hrSummary, kindName, stressAside, stressBuckets, stressSummary } from './model';
import { ChartSummary } from './parts';
import { clock } from './time';

// The cards of one day (heart rate, stress, energy, activities), shared by the Day view and the 1-day
// tab of the metric pages.

export const HOURS = ['00', '06', '12', '18', '24'];
/** The hour labels as the chart's x axis: "00" at hour 0 ... "24" at hour 24. */
export const HOUR_LABELS: XLabel[] = HOURS.map((text, i) => ({ value: i * 6, text }));

export function HeartRateCard({ day }: { day: DayResponse }) {
  const desktop = useIsDesktop();
  const hr = hourSeries(day.date, day.heart_rate.day.series);
  return (
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
  );
}

export function StressCard({ day }: { day: DayResponse }) {
  const st = day.stress;
  const buckets = stressBuckets(day.date, st.series);
  return (
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
  );
}

export function EnergyCard({ day }: { day: DayResponse }) {
  const desktop = useIsDesktop();
  const energy = hourSeries(day.date, day.energy.series);
  const wake = clock(day.energy.wake_time);
  return (
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
  );
}

export function ActivitiesSection({ day }: { day: DayResponse }) {
  const desktop = useIsDesktop();
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
              sub={[clock(a.start), a.distance_m === null ? null : `${(a.distance_m / 1000).toFixed(1)} km`, a.avg_hr_bpm === null ? null : `${fmtInt(a.avg_hr_bpm)} bpm avg`]
                .filter(Boolean)
                .join(' · ')}
              value={fmtDurationMin(a.duration_s === null ? null : a.duration_s / 60)}
            />
          ))}
        </div>
      )}
    </section>
  );
}
