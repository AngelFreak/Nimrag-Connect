import type { CSSProperties, ReactNode } from 'react';
import { Link } from 'react-router-dom';
import type { HomeCard, Today } from '../../api/generated';
import type { StatusKind } from '../../api/generated/StatusKind';
import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import type { NightSummary } from '../../api/generated/NightSummary';
import { RangeBar, Ring, Skeleton, StageBar, StatusWord, Unavailable, ValueOrUnavailable } from '../../components';
import { addDays, fmtMonthDay, fmtShortDay } from '../../lib/dates';
import { fmtFixed, fmtInt, isMissing, statusName } from '../../lib/format';
import { glanceWord, METRIC_SHORT, STATE_WORD } from '../health/healthStatus';
import { useHealthStatus } from '../health/healthStatusQueries';
import { LEVEL_COLOR, LEVEL_NAME } from '../health/readiness';
import { useReadiness } from '../health/readinessQueries';
import { RATING_WORD, headlineFor, ratingOfScore } from '../health/sleepNight';
import { CARD_LABEL, CARD_LINK } from './cards';
import { dayLetter, baselineLabel, fmtKm, fmtPace, fmtSeconds, fmtTime, hrvStatusLabel, signed, stressLevelWord, targetLabel, unit } from './format';
import { Glyph, type GlyphName } from './glyphs';
import { DayArea, DotStrip, MiniBars, Sparkline } from './homeCharts';
import '../health/healthStatus.css';

// The cards of the desktop Home: "In Focus" (332 x 304) and "At a Glance" (185 x 257). Layout from
// docs/design/web-layout/SPEC.md part B; the data is what GET /api/today and the activity list already give.

/** The activities of the recent list, or why there are none yet. */
export type ActivitiesState = { status: 'pending' } | { status: 'error' } | { status: 'ready'; items: ActivityListItem[] };

const LOOKBACK_DAYS = 28;

/** The seven days ending on `date`, oldest first. */
export function weekDays(date: string): string[] {
  return Array.from({ length: 7 }, (_, i) => addDays(date, i - 6));
}

/** "Nov 18 - 24" style range used in the card titles. */
function weekRange(days: string[]): string {
  return `${fmtMonthDay(days[0]!)}–${fmtMonthDay(days[days.length - 1]!)}`;
}

// ---------------------------------------------------------------- shared pieces

function Empty({ glyph, children }: { glyph: GlyphName; children: ReactNode }) {
  return (
    <div className="hempty">
      <span className="hempty__disc">
        <Glyph name={glyph} size={22} />
      </span>
      <p className="hempty__text">{children}</p>
    </div>
  );
}

interface ShellProps {
  id: string;
  size: 'focus' | 'glance';
  glyph: GlyphName;
  /** Colour of the icon: a CSS colour or variable. */
  tone: string;
  title: ReactNode;
  to: string;
  children: ReactNode;
  footer?: ReactNode;
}

/** A card: icon and title (the title is the link; the whole card is clickable), body, optional footer under a rule. */
function Shell({ id, size, glyph, tone, title, to, children, footer }: ShellProps) {
  return (
    <article className={`hcard hcard--${size}`} data-card={id} style={{ '--tone': tone } as CSSProperties}>
      <header className="hcard__head">
        <span className="hcard__icon">
          <Glyph name={glyph} />
        </span>
        <h3 className="hcard__title">
          <Link className="hcard__link" to={to}>
            {title}
          </Link>
        </h3>
      </header>
      <div className="hcard__body">{children}</div>
      {footer ? <footer className="hcard__foot">{footer}</footer> : null}
    </article>
  );
}

function Big({ children }: { children: ReactNode }) {
  return <span className="hbig">{children}</span>;
}

function Side({ items }: { items: { value: ReactNode; label: string }[] }) {
  return (
    <div className="hside">
      {items.map((s) => (
        <span className="hside__item" key={s.label}>
          <span className="hside__v">
            <ValueOrUnavailable value={s.value} />
          </span>
          <span className="hside__l">{s.label}</span>
        </span>
      ))}
    </div>
  );
}

/** Label and value stacked, for the narrow cards. */
function Stat({ value, label, tone }: { value: ReactNode; label: string; tone?: string }) {
  return (
    <div className="hstat">
      <span className="hstat__v" style={tone ? { color: tone } : undefined}>
        <ValueOrUnavailable value={value} />
      </span>
      <span className="hstat__l">{label}</span>
    </div>
  );
}

function Loading() {
  return (
    <div className="hcard__loading" aria-busy="true">
      <Skeleton height={32} width="40%" />
      <Skeleton height={96} />
    </div>
  );
}

// ---------------------------------------------------------------- activities in the last days

function inLast(items: ActivityListItem[], days: string[], kind?: string): ActivityListItem[] {
  const set = new Set(days);
  return items.filter((a) => set.has(a.date) && (kind === undefined || a.kind === kind));
}

function perDay(items: ActivityListItem[], days: string[], pick: (a: ActivityListItem) => number | null): number[] {
  return days.map((d) => items.filter((a) => a.date === d).reduce((sum, a) => sum + (pick(a) ?? 0), 0));
}

/** One dot per day of the last 4 weeks, filled on days with an activity. */
function monthDots(items: ActivityListItem[], date: string, kind?: string): boolean[] {
  const days = new Set(items.filter((a) => kind === undefined || a.kind === kind).map((a) => a.date));
  return Array.from({ length: LOOKBACK_DAYS }, (_, i) => days.has(addDays(date, i - (LOOKBACK_DAYS - 1))));
}

function WeekFooter({ items, date, kind }: { items: ActivityListItem[]; date: string; kind?: string }) {
  const dots = monthDots(items, date, kind);
  const n = dots.filter(Boolean).length;
  return (
    <>
      <DotStrip days={dots} label={`${n} ${n === 1 ? 'day' : 'days'} with ${kind ? 'a run' : 'an activity'} in the last 4 weeks`} />
      <span className="hfoot__label">Last 4w</span>
    </>
  );
}

function ActivityWeekCard({ id, glyph, tone, title, to, kind, date, activities, emptyText }: {
  id: string;
  glyph: GlyphName;
  tone: string;
  title: string;
  to: string;
  kind?: string;
  date: string;
  activities: ActivitiesState;
  emptyText: string;
}) {
  const days = weekDays(date);
  const heading = `${title} · ${weekRange(days)}`;
  if (activities.status !== 'ready') {
    return (
      <Shell id={id} size="focus" glyph={glyph} tone={tone} title={heading} to={to}>
        {activities.status === 'pending' ? <Loading /> : <Empty glyph={glyph}>Could not load your activities.</Empty>}
      </Shell>
    );
  }
  const week = inLast(activities.items, days, kind);
  const footer = <WeekFooter items={activities.items} date={date} {...(kind ? { kind } : {})} />;
  if (week.length === 0) {
    return (
      <Shell id={id} size="focus" glyph={glyph} tone={tone} title={heading} to={to} footer={footer}>
        <Empty glyph={glyph}>{emptyText}</Empty>
      </Shell>
    );
  }
  const seconds = week.reduce((s, a) => s + (a.duration_s ?? 0), 0);
  const meters = week.reduce((s, a) => s + (a.distance_m ?? 0), 0);
  const isRun = kind === 'run';
  const values = isRun ? perDay(activities.items, days, (a) => (a.kind === 'run' ? (a.distance_m ?? 0) / 1000 : 0)) : perDay(activities.items, days, (a) => (a.duration_s ?? 0) / 60);
  const per = days.map((_, i) => (isRun ? `${values[i]!.toFixed(1)} km` : fmtSeconds(values[i]! * 60) ?? '0m'));
  const summary = `${title} per day, ${weekRange(days)}: ${days.map((d, i) => `${fmtShortDay(d)} ${values[i]! > 0 ? per[i] : 'none'}`).join(', ')}.`;
  return (
    <Shell id={id} size="focus" glyph={glyph} tone={tone} title={heading} to={to} footer={footer}>
      <div className="hrow">
        <Big>{isRun ? fmtKm(meters) : fmtSeconds(seconds)}</Big>
        <Side
          items={
            isRun
              ? [
                  { value: fmtSeconds(seconds), label: 'Time' },
                  { value: meters > 0 && seconds > 0 ? fmtPace(seconds / (meters / 1000)) : null, label: 'Avg pace' },
                ]
              : [{ value: String(week.length), label: week.length === 1 ? 'Activity' : 'Activities' }]
          }
        />
      </div>
      <MiniBars values={values} letters={days.map(dayLetter)} color={tone} label={summary} highlight={days.length - 1} height={104} />
    </Shell>
  );
}

// ---------------------------------------------------------------- In Focus

function EnergyCard({ today }: { today: Today }) {
  const e = today.energy;
  const tz = today.timezone;
  const points = e.series.flatMap((p) => (p.value === null ? [] : [{ t: Date.parse(p.time), v: p.value }])).filter((p) => !Number.isNaN(p.t));
  const link = CARD_LINK.energy;
  if (isMissing(e.now) && points.length === 0) {
    return (
      <Shell id="energy" size="focus" glyph="energy" tone="var(--energy)" title={CARD_LABEL.energy} to={link}>
        <Empty glyph="energy">No energy data yet today.</Empty>
      </Shell>
    );
  }
  const t0 = points[0]?.t ?? 0;
  const t1 = points.at(-1)?.t ?? 1;
  return (
    <Shell id="energy" size="focus" glyph="energy" tone="var(--energy)" title={CARD_LABEL.energy} to={link}>
      <div className="hrow">
        <Big>
          <ValueOrUnavailable value={fmtInt(e.now)} />
        </Big>
        <Side
          items={[
            { value: signed(e.charged), label: 'Charged' },
            { value: isMissing(e.drained) ? null : signed(-e.drained), label: 'Drained' },
          ]}
        />
      </div>
      <div className="hcard__chart">
        <DayArea points={points} t0={t0} t1={t1} color="var(--energy)" label={`Energy over the day, from ${fmtInt(e.low) ?? 'unavailable'} to ${fmtInt(e.high) ?? 'unavailable'}, now ${fmtInt(e.now) ?? 'unavailable'}.`} height={112} />
        {points.length > 1 ? (
          <div className="hchart__axis" aria-hidden="true">
            <span>{fmtTime(new Date(t0).toISOString(), tz)}</span>
            <span>{fmtTime(new Date(t1).toISOString(), tz)}</span>
          </div>
        ) : null}
      </div>
    </Shell>
  );
}

function StepsCard({ today }: { today: Today }) {
  const s = today.steps;
  const link = CARD_LINK.steps;
  if (isMissing(s.steps)) {
    return (
      <Shell id="steps" size="focus" glyph="steps" tone="var(--steps)" title={CARD_LABEL.steps} to={link}>
        <Empty glyph="steps">No steps recorded yet today.</Empty>
      </Shell>
    );
  }
  const pct = s.goal > 0 ? Math.round((s.steps / s.goal) * 100) : null;
  return (
    <Shell id="steps" size="focus" glyph="steps" tone="var(--steps)" title={CARD_LABEL.steps} to={link} footer={<span className="hfoot__label">{pct === null ? 'No step goal set' : `${pct}% of your goal`}</span>}>
      <div className="hrow">
        <Big>{fmtInt(s.steps)}</Big>
        <Side
          items={[
            { value: fmtInt(s.goal), label: 'Goal' },
            { value: fmtKm(s.distance_m), label: 'Distance' },
          ]}
        />
      </div>
      <div className="hcard__center">
        <Ring value={s.steps} max={s.goal > 0 ? s.goal : 1} label="Steps toward your goal" color="var(--steps)" size={128} strokeWidth={10}>
          {pct === null ? '' : `${pct}%`}
        </Ring>
      </div>
    </Shell>
  );
}

function SleepCard({ today, nights }: { today: Today; nights: NightSummary[] | undefined }) {
  const s = today.sleep;
  const link = CARD_LINK.sleep;
  if (!s) {
    return (
      <Shell id="sleep" size="focus" glyph="sleep" tone="var(--sleep)" title="Sleep Score" to={link}>
        <Empty glyph="sleep">No sleep detected last night.</Empty>
      </Shell>
    );
  }
  const rating = ratingOfScore(s.score);
  const recent = (nights ?? [])
    .slice()
    .sort((a, b) => a.date.localeCompare(b.date))
    .slice(-7);
  const stagesLine = s.stages_available ? [`Deep ${fmtSeconds(s.deep_s) ?? 'unavailable'}`, `Light ${fmtSeconds(s.light_s) ?? 'unavailable'}`, `REM ${fmtSeconds(s.rem_s) ?? 'unavailable'}`].join(' · ') : null;
  const footer = s.stages_available ? (
    <div className="hstages">
      <StageBar
        height={8}
        label={`Sleep stages: deep ${fmtSeconds(s.deep_s) ?? 'unavailable'}, light ${fmtSeconds(s.light_s) ?? 'unavailable'}, REM ${fmtSeconds(s.rem_s) ?? 'unavailable'}, awake ${fmtSeconds(s.awake_s) ?? 'unavailable'}`}
        segments={[
          { key: 'deep', label: 'Deep', value: s.deep_s, color: 'var(--stage-deep)' },
          { key: 'light', label: 'Light', value: s.light_s, color: 'var(--stage-light)' },
          { key: 'rem', label: 'REM', value: s.rem_s, color: 'var(--stage-rem)' },
          { key: 'awake', label: 'Awake', value: s.awake_s, color: 'var(--stage-awake)' },
        ]}
      />
      <span className="hfoot__label">{stagesLine}</span>
    </div>
  ) : (
    <span className="hfoot__label">Sleep stages unavailable</span>
  );
  return (
    <Shell id="sleep" size="focus" glyph="sleep" tone="var(--sleep)" title="Sleep Score" to={link} footer={footer}>
      <div className="hrow">
        <Big>{isMissing(s.score) ? <Unavailable /> : fmtInt(s.score)}</Big>
        <Side
          items={[
            { value: fmtSeconds(s.duration_s), label: 'Asleep' },
            { value: rating ? <StatusWord type="sleep" value={rating}>{RATING_WORD[rating]}</StatusWord> : null, label: 'Quality' },
          ]}
        />
      </div>
      <p className="hcard__caption">{headlineFor(s).title}</p>
      {recent.length > 0 ? (
        <MiniBars
          values={recent.map((n) => n.score)}
          letters={recent.map((n) => dayLetter(n.date))}
          color="var(--sleep)"
          max={100}
          highlight={recent.length - 1}
          height={96}
          label={`Sleep score of the last ${recent.length} nights: ${recent.map((n) => `${fmtShortDay(n.date)} ${isMissing(n.score) ? 'unavailable' : n.score}`).join(', ')}.`}
        />
      ) : null}
    </Shell>
  );
}

const STATUS_COLOR: Record<StatusKind, string> = {
  detraining: 'var(--status-detraining)',
  maintaining: 'var(--status-maintaining)',
  recovery: 'var(--status-recovery)',
  productive: 'var(--status-productive)',
  strained: 'var(--status-strained)',
  none: 'var(--status-none)',
  unknown: 'var(--status-none)',
};

function TrainingStatusCard({ today }: { today: Today }) {
  const s = today.training_status;
  const l = today.training_load;
  const link = CARD_LINK.training_status;
  const name = statusName(s.label);
  const tone = STATUS_COLOR[s.kind ?? 'none'];
  if (!name) {
    return (
      <Shell id="training_status" size="focus" glyph="status" tone="var(--status-none)" title={CARD_LABEL.training_status} to={link}>
        <Empty glyph="status">No training status yet.</Empty>
      </Shell>
    );
  }
  const since = s.since ? fmtShortDay(s.since.slice(0, 10)) : null;
  const load = l.series_4w.map((d) => d.value);
  return (
    <Shell
      id="training_status"
      size="focus"
      glyph="status"
      tone={tone}
      title={CARD_LABEL.training_status}
      to={link}
      footer={<span className="hfoot__label">{since ? `Since ${since}${s.since_is_lower_bound ? ' or earlier' : ''}` : 'Current status'}</span>}
    >
      <div className="hrow">
        <Big>{name}</Big>
        <Side items={isMissing(l.seven_day_load) ? [] : [{ value: fmtInt(l.seven_day_load), label: '7-day load' }]} />
      </div>
      {load.some((v) => v !== null) ? (
        <div className="hcard__chart">
          <Sparkline values={load} color={tone} area height={104} label="Training load over the last 4 weeks." />
          <div className="hchart__axis" aria-hidden="true">
            <span>4 weeks ago</span>
            <span>Now</span>
          </div>
        </div>
      ) : null}
    </Shell>
  );
}

/** The cards of the "In Focus" grid: the fixed order of the spec, minus those hidden in Edit Home. */
export function FocusCards({ today, hidden, activities, nights }: { today: Today; hidden: readonly HomeCard[]; activities: ActivitiesState; nights: NightSummary[] | undefined }) {
  const shown = (id: HomeCard) => !hidden.includes(id);
  return (
    <>
      {shown('energy') ? <EnergyCard today={today} /> : null}
      <ActivityWeekCard id="week_activities" glyph="activities" tone="var(--zone-1)" title="All activities" to="/activities" date={today.date} activities={activities} emptyText="No activities in the last 7 days." />
      <ActivityWeekCard id="week_running" glyph="run" tone="#f27a1a" title="Running" to="/activities" kind="run" date={today.date} activities={activities} emptyText="No runs this week." />
      {shown('steps') ? <StepsCard today={today} /> : null}
      {shown('sleep') ? <SleepCard today={today} nights={nights} /> : null}
      {shown('training_status') ? <TrainingStatusCard today={today} /> : null}
    </>
  );
}

// ---------------------------------------------------------------- At a Glance

function sparkSummary(what: string, values: (number | null)[], suffix = ''): string {
  const known = values.filter((v): v is number => v !== null);
  return known.length === 0 ? `${what}: no values.` : `${what}: ${known.length} values from ${Math.min(...known)}${suffix} to ${Math.max(...known)}${suffix}.`;
}

function Glance({ id, glyph, tone, children, footer, title }: { id: HomeCard; glyph: GlyphName; tone: string; children: ReactNode; footer?: ReactNode; title?: string }) {
  return (
    <Shell id={id} size="glance" glyph={glyph} tone={tone} title={title ?? CARD_LABEL[id]} to={CARD_LINK[id]} {...(footer ? { footer } : {})}>
      {children}
    </Shell>
  );
}

function GlanceEmpty({ id, glyph, tone, children, title }: { id: HomeCard; glyph: GlyphName; tone: string; children: ReactNode; title?: string }) {
  return (
    <Glance id={id} glyph={glyph} tone={tone} {...(title ? { title } : {})}>
      <Empty glyph={glyph}>{children}</Empty>
    </Glance>
  );
}

/** One small card of "At a Glance" (every layout card that is not in "In Focus"). */
export function HomeGlanceCard({ id, today }: { id: HomeCard; today: Today }) {
  const tz = today.timezone;
  switch (id) {
    case 'heart_rate': {
      const h = today.heart_rate;
      if (isMissing(h.resting_hr_bpm) && h.day.series.length === 0) return <GlanceEmpty id={id} glyph="heart" tone="var(--heart)">No heart rate recorded yet today.</GlanceEmpty>;
      const values = h.day.series.map((p) => p.value);
      return (
        <Glance id={id} glyph="heart" tone="var(--heart)" footer={<span className="hfoot__label">Today</span>}>
          <Stat value={unit(h.resting_hr_bpm, 'bpm')} label="Resting" />
          <Stat value={unit(h.avg_7d_resting_hr_bpm, 'bpm')} label="Avg 7d resting" />
          <div className="hcard__chart hcard__chart--end">
            <Sparkline values={values} color="var(--heart)" height={44} dot={false} {...(isMissing(h.resting_hr_bpm) ? {} : { reference: h.resting_hr_bpm })} label={`${sparkSummary('Heart rate today', values, ' bpm')}${isMissing(h.resting_hr_bpm) ? '' : ` Resting ${h.resting_hr_bpm} bpm.`}`} />
          </div>
        </Glance>
      );
    }
    case 'stress': {
      const s = today.stress;
      if (isMissing(s.average)) return <GlanceEmpty id={id} glyph="stress" tone="var(--stress)">No stress measured yet today.</GlanceEmpty>;
      const word = stressLevelWord(s.main_level);
      return (
        <Glance
          id={id}
          glyph="stress"
          tone="var(--stress)"
          footer={
            <StageBar
              height={8}
              label={`Stress today: rest ${fmtSeconds(s.rest_s) ?? 'unavailable'}, low ${fmtSeconds(s.low_s) ?? 'unavailable'}, medium ${fmtSeconds(s.medium_s) ?? 'unavailable'}, high ${fmtSeconds(s.high_s) ?? 'unavailable'}`}
              segments={[
                { key: 'rest', label: 'Rest', value: s.rest_s, color: 'var(--stress-rest)' },
                { key: 'low', label: 'Low', value: s.low_s, color: 'var(--stress-low)' },
                { key: 'medium', label: 'Medium', value: s.medium_s, color: 'var(--stress-medium)' },
                { key: 'high', label: 'High', value: s.high_s, color: 'var(--stress-high)' },
              ]}
            />
          }
        >
          <div className="hcard__center">
            <Ring value={s.average} label="Average stress" color="var(--stress)" size={92} strokeWidth={8}>
              {fmtInt(s.average)}
            </Ring>
          </div>
          <p className="hcard__caption hcard__caption--center">{word ? `Average · mostly ${word}` : 'Average today'}</p>
        </Glance>
      );
    }
    case 'hrv': {
      const h = today.hrv;
      if (isMissing(h.last_night_ms)) return <GlanceEmpty id={id} glyph="hrv" tone="var(--heart)" title="HRV Status">Wear your watch while sleeping to reveal your status.</GlanceEmpty>;
      const values = h.values_4w.map((d) => d.value);
      const band: [number, number] | undefined = !isMissing(h.baseline_low_ms) && !isMissing(h.baseline_high_ms) ? [h.baseline_low_ms, h.baseline_high_ms] : undefined;
      return (
        <Glance id={id} glyph="hrv" tone="var(--heart)" title="HRV Status" footer={<span className="hfoot__label">Last 4w</span>}>
          <Stat value={unit(h.last_night_ms, 'ms')} label="Overnight" />
          <p className="hcard__caption">{[baselineLabel(h.vs_baseline), hrvStatusLabel(h.status)].filter(Boolean).join(' · ') || (isMissing(h.weekly_avg_ms) ? '' : `7-night average ${fmtInt(h.weekly_avg_ms)} ms`)}</p>
          <div className="hcard__chart hcard__chart--end">
            <Sparkline values={values} color="var(--link)" height={48} {...(band ? { band } : {})} label={`${sparkSummary('Overnight HRV over the last 4 weeks', values, ' ms')}${band ? ` Usual range ${band[0]} to ${band[1]} ms.` : ''}`} />
          </div>
        </Glance>
      );
    }
    case 'intensity_minutes': {
      const m = today.intensity_minutes;
      if (isMissing(m.total_min)) return <GlanceEmpty id={id} glyph="intensity" tone="var(--warn)">No intensity minutes this week.</GlanceEmpty>;
      return (
        <Glance id={id} glyph="intensity" tone="var(--warn)" footer={<span className="hfoot__label">This week · goal {fmtInt(m.goal_min)}</span>}>
          <div className="hcard__center">
            <Ring value={m.total_min} max={m.goal_min > 0 ? m.goal_min : 1} label="Intensity minutes this week" color="var(--warn)" size={92} strokeWidth={8} />
          </div>
          <p className="hcard__caption hcard__caption--center">
            {[isMissing(m.moderate_min) ? null : `${fmtInt(m.moderate_min)} moderate`, isMissing(m.vigorous_min) ? null : `${fmtInt(m.vigorous_min)} vigorous`].filter(Boolean).join(' · ')}
          </p>
        </Glance>
      );
    }
    case 'training_load': {
      const l = today.training_load;
      if (isMissing(l.seven_day_load)) return <GlanceEmpty id={id} glyph="load" tone="var(--zone-1)">No training load yet.</GlanceEmpty>;
      const values = l.series_4w.map((d) => d.value);
      const range = !isMissing(l.target_min) && !isMissing(l.target_max) ? `Target ${fmtInt(l.target_min)} to ${fmtInt(l.target_max)}` : null;
      return (
        <Glance id={id} glyph="load" tone="var(--zone-1)" footer={<span className="hfoot__label">Last 4w</span>}>
          <Stat value={targetLabel(l.vs_target)?.replace(' range', '') ?? null} label={range ?? 'Target range'} />
          <Stat value={fmtInt(l.seven_day_load)} label="7-day load" />
          {isMissing(l.target_min) || isMissing(l.target_max) ? null : (
            <div className="hcard__range">
              <RangeBar value={l.seven_day_load} min={0} max={Math.max(l.target_max, l.seven_day_load, 1) * 1.4} target={[l.target_min, l.target_max]} label={`7-day load of ${fmtInt(l.seven_day_load)} against a target range of ${fmtInt(l.target_min)} to ${fmtInt(l.target_max)}`} />
            </div>
          )}
          <div className="hcard__chart hcard__chart--end">
            <Sparkline values={values} color="var(--zone-1)" height={40} dot={false} label={sparkSummary('7-day training load over the last 4 weeks', values)} />
          </div>
        </Glance>
      );
    }
    case 'recovery_time': {
      const r = today.recovery_time;
      if (isMissing(r.remaining_s)) return <GlanceEmpty id={id} glyph="recovery" tone="var(--zone-2)">No recovery time from the watch yet.</GlanceEmpty>;
      const ready = fmtTime(r.ready_at, tz);
      return (
        <Glance id={id} glyph="recovery" tone="var(--zone-2)">
          <Stat value={r.remaining_s === 0 ? 'Recovered' : fmtSeconds(r.remaining_s)} label={r.remaining_s === 0 ? 'Ready to train' : ready ? `Ready around ${ready}` : 'Remaining'} />
          {isMissing(r.percent_complete) || r.remaining_s === 0 ? null : (
            <div className="hcard__center hcard__center--grow">
              <Ring value={100 - r.percent_complete} label="Recovery time left" color="var(--zone-2)" size={92} strokeWidth={8} showValue={false} />
            </div>
          )}
        </Glance>
      );
    }
    case 'vo2_max': {
      const v = today.vo2max;
      if (isMissing(v.value)) return <GlanceEmpty id={id} glyph="vo2" tone="var(--zone-4)">No VO2 max reading yet.</GlanceEmpty>;
      const change = signed(v.change_3m, 1);
      const values = v.series_3m.map((d) => d.value);
      return (
        <Glance id={id} glyph="vo2" tone="var(--zone-4)" footer={<span className="hfoot__label">Last 3 months</span>}>
          <Stat value={fmtFixed(v.value, 1)} label="ml/kg/min, derived" />
          <p className="hcard__caption">{change ? `${change} in 3 months` : ''}</p>
          <div className="hcard__chart hcard__chart--end">
            <Sparkline values={values} color="var(--zone-4)" height={44} label={sparkSummary('VO2 max over the last 3 months', values)} />
          </div>
        </Glance>
      );
    }
    case 'respiration': {
      const r = today.respiration;
      if (isMissing(r.latest_brpm)) return <GlanceEmpty id={id} glyph="breathing" tone="var(--zone-1)">No breathing rate yet.</GlanceEmpty>;
      const asleep = fmtFixed(r.sleep_avg_brpm, 1);
      return (
        <Glance id={id} glyph="breathing" tone="var(--zone-1)">
          <Stat value={fmtFixed(r.latest_brpm, 1)} label="breaths a minute" />
          {asleep ? <Stat value={asleep} label="Asleep" /> : null}
        </Glance>
      );
    }
    case 'spo2': {
      const o = today.spo2;
      if (isMissing(o.latest_percent)) return <GlanceEmpty id={id} glyph="spo2" tone="var(--zone-1)">No blood oxygen reading yet.</GlanceEmpty>;
      return (
        <Glance id={id} glyph="spo2" tone="var(--zone-1)">
          <Stat value={`${fmtFixed(o.latest_percent, 0)}%`} label="Latest" />
          {isMissing(o.sleep_low_percent) ? null : <Stat value={`${fmtInt(o.sleep_low_percent)}%`} label="Lowest asleep" />}
        </Glance>
      );
    }
    case 'weight': {
      const w = today.weight;
      if (isMissing(w.weight_kg)) return <GlanceEmpty id={id} glyph="weight" tone="var(--text-2)">No weight logged yet.</GlanceEmpty>;
      const change = signed(w.change_30d_kg, 1);
      return (
        <Glance id={id} glyph="weight" tone="var(--text-2)">
          <Stat value={unit(w.weight_kg, 'kg', 1)} label="Latest" />
          {change ? <Stat value={`${change} kg`} label="In 30 days" /> : null}
        </Glance>
      );
    }
    case 'training_readiness':
      return <ReadinessGlance />;
    case 'health_status':
      return <HealthStatusGlance />;
    case 'energy':
    case 'steps':
    case 'sleep':
    case 'training_status':
      // These four are cards of "In Focus".
      return null;
  }
}

/** Readiness: the estimate's ring, its level word and headline. Nothing is shown without an estimate. */
function ReadinessGlance() {
  const q = useReadiness();
  const r = q.data;
  if (q.isPending) {
    return (
      <Glance id="training_readiness" glyph="readiness" tone="var(--zone-2)">
        <Loading />
      </Glance>
    );
  }
  if (!r || r.score === null || !r.level) return null;
  return (
    <Glance id="training_readiness" glyph="readiness" tone={LEVEL_COLOR[r.level]} footer={<span className="hfoot__label">Nimrag estimate</span>}>
      <div className="hcard__center">
        <Ring value={r.score} label="Readiness estimate" color={LEVEL_COLOR[r.level]} size={92} strokeWidth={8} />
      </div>
      <p className="hcard__caption hcard__caption--center">
        <StatusWord type="readiness" value={r.level}>
          {LEVEL_NAME[r.level]}
        </StatusWord>
        {r.headline ? <span className="hcard__sub">{r.headline}</span> : null}
      </p>
    </Glance>
  );
}

/** Health status: last night's overnight values against your usual range. */
function HealthStatusGlance() {
  const q = useHealthStatus();
  const r = q.data;
  if (q.isPending) {
    return (
      <Glance id="health_status" glyph="heart" tone="var(--heart)">
        <Loading />
      </Glance>
    );
  }
  if (!r) return <GlanceEmpty id="health_status" glyph="heart" tone="var(--heart)">Could not be loaded.</GlanceEmpty>;
  if (!r.has_night) return <GlanceEmpty id="health_status" glyph="heart" tone="var(--heart)">No night recorded yet.</GlanceEmpty>;
  const word = glanceWord(r);
  return (
    <Glance id="health_status" glyph="heart" tone="var(--heart)" footer={<span className="hfoot__label">Last night against your usual</span>}>
      <p className="hcard__caption">
        <StatusWord type="health" value={word.state} size="lg">
          {word.text}
        </StatusWord>
      </p>
      <ul className="hs-list" aria-label="Last night against your usual range">
        {r.metrics.map((m) => (
          <li key={m.key}>
            <span className="hs-list__name">{METRIC_SHORT[m.key]}</span>
            <StatusWord type="health" value={m.state}>
              {STATE_WORD[m.state]}
            </StatusWord>
          </li>
        ))}
      </ul>
    </Glance>
  );
}
