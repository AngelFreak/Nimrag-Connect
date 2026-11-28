import type { ReactNode } from 'react';
import type { DayResponse } from '../../api/generated/DayResponse';
import type { MovementResponse } from '../../api/generated/MovementResponse';
import { BarSeries, Legend, Ring, SampleDataChip, SeriesChart, StatStrip, StatTile, EmptyNote } from '../../components';
import { fmtDay, fmtMonthDay } from '../../lib/dates';
import { fmtFixed, fmtInt, withUnit } from '../../lib/format';
import { useIsDesktop } from '../../lib/hooks';
import { fmtKcal, fmtKm } from '../body/movement';
import { useMe } from '../../account/queries';
import { ActivitiesSection, HOUR_LABELS, HeartRateCard, Panel, StressCard } from '../health/dayCards';
import { StatsColumns, StatsSection } from '../health/desktop';
import { energyAside, fmtSeconds, hourSeries, kindName, stepsPercent } from '../health/model';
import { EnergyDayChart } from '../today/EnergyDayChart';
import { signed } from '../today/format';
import '../today/today.css';
import { CardSkeleton, LoadError } from '../health/parts';
import { useDay } from '../health/queries';
import { clock } from '../health/time';
import { levelSeconds } from '../../lib/stress';
import { weekLine } from './model';
import { daySentence, energyPoints } from './energyDay';
import { StressLevelsPanel } from './StressLevels';
import type { MetricKey } from './period';
import { useMovementDay } from './queries';

// The 1-day tab of a metric page: the day's own data (GET /api/days/{date}), with the day's
// summary figures beside it. Heart rate, stress and energy draw the same cards as the Day view.

type Tone = 'energy' | 'sleep' | 'heart' | 'steps' | 'warn' | 'link' | 'muted';
interface TileSpec {
  label: string;
  value: string | null;
  tone: Tone;
  sub?: string;
}
const tile = (label: string, value: string | null, tone: Tone = 'muted', sub?: string): TileSpec => ({ label, value, tone, ...(sub ? { sub } : {}) });

/** The chart, and the day's figures: tiles beside it on a phone, a strip in the 320 px column on a wide screen. */
function Layout({ chart, tiles, children, emptyText = 'Nothing recorded for this day.' }: { chart: ReactNode; tiles: TileSpec[]; children?: ReactNode; emptyText?: string }) {
  const desktop = useIsDesktop();
  // A figure the watch did not record is left out; when none was, one sentence says so.
  const shown = tiles.filter((t) => t.value !== null);
  if (desktop) {
    return (
      <StatsColumns
        side={
          <StatsSection title="Summary">
            {shown.length === 0 ? <EmptyNote>{emptyText}</EmptyNote> : <StatStrip items={shown.map((t) => ({ key: t.label, label: t.sub ? `${t.label}, ${t.sub}` : t.label, value: t.value }))} />}
          </StatsSection>
        }
      >
        {chart}
        {children}
      </StatsColumns>
    );
  }
  return (
    <>
      <div className="metric-top">
        {chart}
        <div className="metric-tiles">
          {shown.length === 0 ? <EmptyNote>{emptyText}</EmptyNote> : null}
          {shown.map((t) => (
            <StatTile key={t.label} size="sm" label={t.label} value={t.value} tone={t.tone} {...(t.sub ? { sub: t.sub } : {})} />
          ))}
        </div>
      </div>
      {children}
    </>
  );
}

export function DayBody({ metric, date, today }: { metric: MetricKey; date: string; today: string }) {
  const q = useDay(date);
  if (q.isPending) {
    return (
      <>
        <CardSkeleton lines={4} height={20} />
        <CardSkeleton lines={2} height={20} />
      </>
    );
  }
  if (q.isError) return <LoadError what="this day" error={q.error} onRetry={() => void q.refetch()} />;
  const day = q.data;
  return (
    <div className="page-stack">
      {day.sample ? (
        <div>
          <SampleDataChip sample />
        </div>
      ) : null}
      {metric === 'heart_rate' ? (
        <HeartRateDay day={day} />
      ) : metric === 'stress' ? (
        <StressDay day={day} />
      ) : metric === 'energy' ? (
        <EnergyDay day={day} today={today} />
      ) : metric === 'steps' ? (
        <StepsDay day={day} />
      ) : metric === 'intensity_minutes' ? (
        <IntensityDay day={day} today={today} />
      ) : metric === 'respiration' ? (
        <BreathingDay day={day} />
      ) : metric === 'spo2' ? (
        <Spo2Day day={day} />
      ) : null}
    </div>
  );
}

function HeartRateDay({ day }: { day: DayResponse }) {
  const hr = day.heart_rate;
  return (
    <Layout
      chart={<HeartRateCard day={day} />}
      tiles={[
        tile('Resting', withUnit(fmtInt(hr.resting_hr_bpm), 'bpm'), 'heart'),
        tile('Highest', withUnit(fmtInt(hr.day.high_bpm), 'bpm')),
        tile('Lowest', withUnit(fmtInt(hr.day.low_bpm), 'bpm')),
        tile('7-day resting', withUnit(fmtInt(hr.avg_7d_resting_hr_bpm), 'bpm')),
      ]}
    >
      <ActivitiesSection day={day} />
    </Layout>
  );
}

function StressDay({ day }: { day: DayResponse }) {
  const st = day.stress;
  return (
    <Layout
      chart={
        <div className="page-stack">
          <StressCard day={day} ranges />
          <StressLevelsPanel secs={levelSeconds(st)} />
        </div>
      }
      tiles={[
        tile('Average', fmtInt(st.average), 'warn'),
        tile('Rest', fmtSeconds(st.rest_s)),
        tile('Low', fmtSeconds(st.low_s)),
        tile('Medium', fmtSeconds(st.medium_s)),
        tile('High', fmtSeconds(st.high_s)),
        tile('Activity', fmtSeconds(st.activity_s)),
      ]}
    >
      <ActivitiesSection day={day} />
    </Layout>
  );
}

function EnergyDay({ day, today }: { day: DayResponse; today: string }) {
  const e = day.energy;
  const desktop = useIsDesktop();
  const me = useMe();
  const timezone = me.data?.timezone ?? 'UTC';
  const isToday = day.date === today;
  const points = energyPoints(e.series);
  const sleep = day.sleep ? { start: day.sleep.start, end: day.sleep.end } : null;
  const activities = day.activities.map((a) => ({
    id: a.id,
    kind: a.kind,
    name: a.sport_name && a.kind === 'other' ? a.sport_name : kindName(a.kind),
    start: a.start,
    duration_s: a.duration_s,
  }));
  const sentence = daySentence({
    points,
    sleep: sleep ? { start: Date.parse(sleep.start), end: Date.parse(sleep.end) } : null,
    activities: activities.flatMap((a) => {
      const start = Date.parse(a.start);
      return Number.isNaN(start) || a.duration_s === null ? [] : [{ name: a.name, start, end: start + a.duration_s * 1000 }];
    }),
    stressAverage: day.stress.average,
    isToday,
    now: e.now,
  });
  const flow = (v: number | null, sign: 1 | -1) => (v === null ? null : signed(sign * v));
  return (
    <Layout
      chart={
        <Panel title="Energy" aside={energyAside(e) ?? undefined}>
          <EnergyDayChart energy={e} stress={day.stress} timezone={timezone} sleep={sleep} activities={activities} tall={desktop} />
          {sentence ? <p className="day-sentence">{sentence}</p> : null}
        </Panel>
      }
      tiles={[
        ...(isToday ? [tile('Now', fmtInt(e.now), 'energy')] : []),
        tile(isToday ? 'Charged today' : 'Charged', flow(e.charged, 1), 'energy'),
        tile(isToday ? 'Drained today' : 'Drained', flow(e.drained, -1), 'warn'),
        // Four tiles, two rows: today "Now" takes the place of the highest (the header has both ends).
        ...(isToday ? [] : [tile('Highest', fmtInt(e.high), 'energy')]),
        tile('Lowest', fmtInt(e.low), 'warn'),
      ]}
    >
      <ActivitiesSection day={day} />
    </Layout>
  );
}

// ---- steps ------------------------------------------------------------------------------------------

function StepsDay({ day }: { day: DayResponse }) {
  const m = useMovementDay(day.date, true);
  const pct = stepsPercent(day.steps, day.step_goal);
  const move: MovementResponse | undefined = m.data;
  return (
    <Layout
      chart={
        <Panel title="Steps today" padding="lg" muted>
          <div className="metric-ring">
            <Ring value={day.steps} max={day.step_goal} label="Steps against the daily goal" color="var(--steps)" size={112} strokeWidth={12} showValue={false} />
            <div className="stat">
              {fmtInt(day.steps) === null ? <EmptyNote>No steps recorded for this day.</EmptyNote> : <span className="stat__value">{fmtInt(day.steps)}</span>}
              <span className="stat__sub tone-steps">{pct === null ? `Goal ${fmtInt(day.step_goal)}` : `${pct}% of ${fmtInt(day.step_goal)}`}</span>
            </div>
          </div>
        </Panel>
      }
      tiles={[tile('Distance', fmtKm(move?.distance_m)), tile('Calories', fmtKcal(move?.calories.total_kcal))]}
    >
      {move ? <WeekSteps move={move} /> : null}
    </Layout>
  );
}

function WeekSteps({ move }: { move: MovementResponse }) {
  const desktop = useIsDesktop();
  const values = move.week_steps.map((d) => d.value);
  return (
    <Panel title="This week" padding="lg" muted>
      <BarSeries
        values={values}
        labels={move.week_steps.map((d) => fmtDay(d.date, { weekday: 'short' }))}
        label="Steps per day this week"
        summary={`Steps per day this week: ${move.week_steps.map((d) => `${fmtDay(d.date, { weekday: 'long' })} ${fmtInt(d.value) ?? 'no data'}`).join(', ')}.`}
        color="var(--steps)"
        belowGoalColor="var(--steps-dim)"
        goal={move.step_goal}
        height={desktop ? 200 : 110}
      />
      <Legend
        items={[
          { key: 'met', label: 'Goal reached', color: 'var(--steps)' },
          { key: 'below', label: 'Below goal', color: 'var(--steps-dim)' },
          { key: 'goal', label: 'Goal', color: 'var(--text-2)', shape: 'dash' },
        ]}
      />
    </Panel>
  );
}

// ---- intensity minutes ----------------------------------------------------------------------------

function IntensityDay({ day, today }: { day: DayResponse; today: string }) {
  const m = useMovementDay(day.date, true);
  const desktop = useIsDesktop();
  const week = m.data?.intensity;
  const dayFigures = week?.days.find((d) => d.date === day.date);
  const spec = week
    ? weekLine(
        week.days.map((d) => d.date),
        week.days.map((d) => d.total_min),
        week.goal_min,
        today,
        [0, 2, 4, 6].map((i) => ({ value: i, text: fmtMonthDay(week.days[i]?.date ?? week.week_start) })),
      )
    : null;
  const pct = week && week.total_min !== null && week.goal_min > 0 ? Math.round((week.total_min / week.goal_min) * 100) : null;
  return (
    <Layout
      chart={
        <Panel title="Week so far" padding="lg" muted>
          <div className="metric-ring">
            <Ring value={week?.total_min ?? null} max={week?.goal_min ?? 150} label="Intensity minutes against the weekly goal" color="var(--energy)" size={112} strokeWidth={12} showValue={false} />
            <div className="stat">
              {withUnit(fmtInt(week?.total_min), 'min') === null ? (
                <EmptyNote>No intensity minutes recorded this week.</EmptyNote>
              ) : (
                <span className="stat__value">{withUnit(fmtInt(week?.total_min), 'min')}</span>
              )}
              <span className="stat__sub tone-energy">{pct === null ? '' : `${pct}% of ${fmtInt(week?.goal_min)}`}</span>
            </div>
          </div>
          {spec ? (
            <>
              <SeriesChart
                label="Intensity minutes through the week"
                summary={`Intensity minutes this week: ${fmtInt(week?.total_min) ?? 'none recorded'} of ${fmtInt(week?.goal_min)}.`}
                x={spec.x}
                series={spec.series}
                xRange={spec.xRange}
                xLabels={spec.xLabels}
                {...(spec.yRange ? { yRange: spec.yRange } : {})}
                height={desktop ? 220 : 120}
              />
              <Legend items={spec.legend} />
            </>
          ) : null}
        </Panel>
      }
      tiles={[
        tile('Moderate', withUnit(fmtInt(dayFigures?.moderate_min), 'min'), 'energy'),
        tile('Vigorous', withUnit(fmtInt(dayFigures?.vigorous_min), 'min'), 'energy', 'counts double'),
        tile('Credited', withUnit(fmtInt(dayFigures?.total_min ?? day.intensity_minutes), 'min')),
      ]}
      emptyText="No intensity minutes recorded for this day."
    />
  );
}

// ---- breathing and blood oxygen -----------------------------------------------------------------

const brpm = (v: number | null) => withUnit(fmtFixed(v, 1), 'brpm');

function BreathingDay({ day }: { day: DayResponse }) {
  const desktop = useIsDesktop();
  const b = day.breathing;
  const hours = hourSeries(day.date, b.hourly);
  const summary =
    b.lowest === null || b.highest === null
      ? 'No breathing readings for this day.'
      : `Breathing rate by the hour, from ${fmtFixed(b.lowest, 1)} to ${fmtFixed(b.highest, 1)} breaths a minute.`;
  if (hours === null && [b.lowest, b.highest, b.avg_awake, b.avg_asleep].every((v) => v === null)) {
    return (
      <Panel title="Breathing by the hour">
        <EmptyNote>No breathing readings for this day.</EmptyNote>
      </Panel>
    );
  }
  return (
    <Layout
      chart={
        <Panel title="Breathing by the hour" aside={b.avg_awake === null ? undefined : `Awake ${fmtFixed(b.avg_awake, 1)}`}>
          <SeriesChart
            label="Breathing rate across the day"
            summary={summary}
            summaryVisible
            x={hours?.x ?? []}
            xRange={[0, 24]}
            series={[{ label: 'Breathing', color: 'link', data: hours?.y ?? [], points: true }]}
            yUnit="decimal"
            height={desktop ? 220 : 120}
            xLabels={HOUR_LABELS}
          />
        </Panel>
      }
      tiles={[tile('Lowest', brpm(b.lowest)), tile('Highest', brpm(b.highest)), tile('Awake avg', brpm(b.avg_awake), 'link'), tile('Asleep avg', brpm(b.avg_asleep), 'sleep')]}
    />
  );
}

function Spo2Day({ day }: { day: DayResponse }) {
  const desktop = useIsDesktop();
  const o = day.spo2;
  const hours = hourSeries(day.date, o.hourly);
  const low = o.hourly.flatMap((h) => (h.value === null ? [] : [h.value]));
  const floor = low.length ? Math.min(90, Math.floor(Math.min(...low) / 5) * 5) : 90;
  const pct = (v: number | null) => (v === null ? null : `${fmtInt(v)}%`);
  const summary =
    o.lowest === null || o.average === null
      ? 'No blood oxygen readings for this day.'
      : `Blood oxygen by the hour: average ${fmtInt(o.average)}%, lowest ${fmtInt(o.lowest)}%.`;
  const at = clock(o.latest_at);
  if (!low.length && [o.average, o.lowest, o.latest, o.avg_asleep].every((v) => v === null)) {
    return (
      <Panel title="Blood oxygen by the hour">
        <EmptyNote>No blood oxygen readings for this day. Turn on Pulse Ox on the watch to record them.</EmptyNote>
      </Panel>
    );
  }
  return (
    <Layout
      chart={
        <Panel title="Blood oxygen by the hour" aside={o.average === null ? undefined : `Avg ${fmtInt(o.average)}%`}>
          <SeriesChart
            label="Blood oxygen across the day"
            summary={summary}
            summaryVisible
            x={hours?.x ?? []}
            xRange={[0, 24]}
            series={[{ label: 'Blood oxygen', color: 'link', data: hours?.y ?? [], points: true }]}
            yRange={[floor, 100]}
            height={desktop ? 220 : 120}
            xLabels={HOUR_LABELS}
          />
        </Panel>
      }
      tiles={[
        tile('Average', pct(o.average), 'link'),
        tile('Lowest', pct(o.lowest), 'warn'),
        tile('Last reading', pct(o.latest), 'muted', at ? `at ${at}` : undefined),
        tile('Asleep avg', pct(o.avg_asleep), 'sleep'),
      ]}
    />
  );
}
