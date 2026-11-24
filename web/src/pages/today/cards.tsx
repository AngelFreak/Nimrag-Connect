import type { ReactNode } from 'react';
import type { HomeCard, HomeCards, Today } from '../../api/generated';
import { ProgressBar, SeriesChart, Skeleton, StageBar, StatTile, StatusWord } from '../../components';
import { fmtShortDay } from '../../lib/dates';
import { glanceWord, METRIC_SHORT, STATE_WORD } from '../health/healthStatus';
import { useHealthStatus } from '../health/healthStatusQueries';
import '../health/healthStatus.css';
import { HeartRateVisual, IntensityVisual, LoadVisual, RecoveryVisual, StatusStripVisual, StepsWeekVisual, StressVisual, Vo2Visual } from './glanceVisuals';
import { fmtFixed, fmtInt, isMissing, statusName } from '../../lib/format';
import {
  baselineLabel,
  fmtSeconds,
  fmtTime,
  hrvStatusLabel,
  signed,
  stressLevelWord,
  unit,
} from './format';

/** Name shown on the card and in the Edit home list. Our words, not the watch maker's. */
export const CARD_LABEL: Record<HomeCard, string> = {
  energy: 'Energy',
  heart_rate: 'Heart rate',
  stress: 'Stress',
  sleep: 'Sleep',
  hrv: 'Overnight HRV',
  training_status: 'Training status',
  intensity_minutes: 'Intensity minutes',
  training_load: 'Training load',
  steps: 'Steps',
  recovery_time: 'Recovery time',
  vo2_max: 'VO2 max',
  respiration: 'Breathing',
  spo2: 'Blood oxygen',
  weight: 'Body weight',
  training_readiness: 'Readiness',
  health_status: 'Health status',
};

/** Detail page each card opens. */
export const CARD_LINK: Record<HomeCard, string> = {
  energy: '/metrics/energy',
  heart_rate: '/metrics/heart_rate',
  stress: '/metrics/stress',
  sleep: '/sleep',
  hrv: '/hrv',
  training_status: '/training/status',
  intensity_minutes: '/metrics/intensity_minutes',
  training_load: '/metrics/training_load',
  steps: '/metrics/steps',
  // Recovery time is part of Training readiness; it has no page of its own.
  recovery_time: '/training',
  vo2_max: '/metrics/vo2_max',
  respiration: '/metrics/respiration',
  spo2: '/metrics/spo2',
  weight: '/body',
  training_readiness: '/training/readiness',
  health_status: '/health/status',
};

/** The server's default layout (settings.rs), used by "Reset to default". */
export const DEFAULT_HOME_CARDS: HomeCards = {
  order: [
    'energy',
    'training_readiness',
    'sleep',
    'heart_rate',
    'stress',
    'hrv',
    'training_status',
    'intensity_minutes',
    'training_load',
    'steps',
    'recovery_time',
    'vo2_max',
    'respiration',
    'spo2',
    'weight',
    'health_status',
  ],
  hidden: ['respiration', 'spo2', 'weight'],
};

/**
 * Cards whose metric the "In focus" carousel already shows (Energy, Readiness, Sleep last night,
 * Overnight HRV); "At a glance" leaves them out so no metric appears twice.
 */
export const IN_FOCUS_CARDS: readonly HomeCard[] = ['energy', 'training_readiness', 'sleep', 'hrv'];

/** The cards to show, in the owner's order, without the hidden ones. */
export function visibleCards(cards: HomeCards): HomeCard[] {
  const hidden = new Set(cards.hidden);
  return cards.order.filter((id) => !hidden.has(id));
}

function join(...parts: (string | null | undefined | false)[]): string | null {
  // Spaces inside a part do not break, so a range like "45–74 today" is never split across lines.
  const present = parts.filter((p): p is string => typeof p === 'string' && p.length > 0).map((p) => p.replace(/ /g, '\u00a0'));
  return present.length > 0 ? present.join(' · ') : null;
}

interface Built {
  value: ReactNode;
  /** The value is a word ("Productive"), which may shrink to fit the half-width tile. */
  word?: boolean;
  /** One short muted line under the value. */
  sub?: ReactNode;
  /** The line is a warning: something is out of range. Only then is it drawn in the alert colour. */
  alert?: boolean;
  extra?: ReactNode;
}

function build(id: HomeCard, t: Today): Built {
  const tz = t.timezone;
  switch (id) {
    case 'energy':
      return {
        value: fmtInt(t.energy.now),
        sub: join(
          isMissing(t.energy.charged) ? null : `${signed(t.energy.charged)} charged`,
          isMissing(t.energy.drained) ? null : `${signed(-t.energy.drained)} drained`,
        ),
      };
    case 'heart_rate': {
      const lo = fmtInt(t.heart_rate.day.low_bpm);
      const hi = fmtInt(t.heart_rate.day.high_bpm);
      return { value: unit(t.heart_rate.resting_hr_bpm, 'bpm'), sub: join('Resting', lo && hi ? `${lo}–${hi} today` : null), extra: <HeartRateVisual today={t} /> };
    }
    case 'stress': {
      const word = stressLevelWord(t.stress.main_level);
      const level = t.stress.main_level;
      return {
        value: fmtInt(t.stress.average),
        sub: word ? (
          <StatusWord type="stress" value={level}>
            Mostly {word}
          </StatusWord>
        ) : (
          'Average'
        ),
        extra: <StressVisual today={t} />,
      };
    }
    case 'sleep': {
      const s = t.sleep;
      return {
        value: fmtSeconds(s?.duration_s),
        sub: s ? join(isMissing(s.score) ? null : `Score ${fmtInt(s.score)}`, s.score_label) : null,
        extra:
          s?.stages_available === true ? (
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
          ) : null,
      };
    }
    case 'hrv': {
      const h = t.hrv;
      const values = h.values_4w.map((d) => d.value);
      const band: [number, number] | undefined =
        !isMissing(h.baseline_low_ms) && !isMissing(h.baseline_high_ms) ? [h.baseline_low_ms, h.baseline_high_ms] : undefined;
      return {
        value: unit(h.last_night_ms, 'ms'),
        sub: h.status && hrvStatusLabel(h.status) ? <StatusWord type="hrv" value={h.status}>{hrvStatusLabel(h.status)}</StatusWord> : baselineLabel(h.vs_baseline),
        alert: h.vs_baseline === 'below',
        extra:
          values.some((v) => v !== null) ? (
            <SeriesChart
              label="Overnight HRV, last 4 weeks"
              summary={`Overnight HRV over the last 4 weeks, ${values.filter((v) => v !== null).length} nights${band ? `, usual range ${band[0]} to ${band[1]} milliseconds` : ''}.`}
              x={values.map((_, i) => i)}
              series={[{ label: 'HRV', color: 'link', data: values, width: 2 }]}
              {...(band ? { band } : {})}
              sparkline
              height={36}
            />
          ) : null,
      };
    }
    case 'training_status': {
      const s = t.training_status;
      const since = s.since ? fmtShortDay(s.since.slice(0, 10)) : null;
      const name = statusName(s.label);
      return {
        value: name ? <StatusWord type="training" value={s.kind}>{name}</StatusWord> : null,
        word: true,
        sub: s.label ? join(since ? `Since ${since}${s.since_is_lower_bound ? ' or earlier' : ''}` : null) : null,
        extra: s.label ? <StatusStripVisual today={t} /> : null,
      };
    }
    case 'training_readiness':
    case 'health_status':
      // Readiness is a slide of "In focus" and Health status has its own tile (HealthStatusTile).
      return { value: null };
    case 'intensity_minutes': {
      const m = t.intensity_minutes;
      return {
        value: unit(m.total_min, 'min'),
        sub: `Goal ${fmtInt(m.goal_min)} this week`,
        extra: <IntensityVisual today={t} />,
      };
    }
    case 'training_load': {
      const l = t.training_load;
      const range = !isMissing(l.target_min) && !isMissing(l.target_max) ? `${fmtInt(l.target_min)}–${fmtInt(l.target_max)}` : null;
      const off = l.vs_target === 'above' || l.vs_target === 'below';
      const where = l.vs_target === 'within' ? 'In range' : l.vs_target === 'above' ? 'Above range' : l.vs_target === 'below' ? 'Below range' : null;
      return { value: fmtInt(l.seven_day_load), sub: join(where, range), alert: off, extra: <LoadVisual today={t} /> };
    }
    case 'steps': {
      const s = t.steps;
      return {
        value: fmtInt(s.steps),
        sub: `Goal ${fmtInt(s.goal)}`,
        extra: (
          <>
            <ProgressBar value={s.steps} max={s.goal} label={`${fmtInt(s.steps) ?? 'unavailable'} of ${fmtInt(s.goal)} steps`} />
            {isMissing(s.steps) ? null : <StepsWeekVisual today={t} />}
          </>
        ),
      };
    }
    case 'recovery_time': {
      const r = t.recovery_time;
      // Nothing left: "0m, ready around <now>" reads like a countdown that is stuck.
      if (r.remaining_s === 0) return { value: 'Recovered', word: true, sub: 'No recovery time left' };
      const ready = fmtTime(r.ready_at, tz);
      return { value: fmtSeconds(r.remaining_s), sub: ready ? `Ready around ${ready}` : null, extra: <RecoveryVisual today={t} /> };
    }
    case 'vo2_max': {
      const v = t.vo2max;
      const change = signed(v.change_3m, 1);
      return { value: fmtFixed(v.value, 1), sub: join('ml/kg/min', change ? `${change} in 3 mo` : null), extra: <Vo2Visual today={t} /> };
    }
    case 'respiration': {
      const r = t.respiration;
      const asleep = fmtFixed(r.sleep_avg_brpm, 1);
      return { value: fmtFixed(r.latest_brpm, 1), sub: join('breaths/min', asleep ? `${asleep} asleep` : null) };
    }
    case 'spo2': {
      const o = t.spo2;
      const low = fmtInt(o.sleep_low_percent);
      return { value: isMissing(o.latest_percent) ? null : `${fmtFixed(o.latest_percent, 0)}%`, sub: low ? `Low ${low}% asleep` : null };
    }
    case 'weight': {
      const w = t.weight;
      const change = signed(w.change_30d_kg, 1);
      return { value: unit(w.weight_kg, 'kg', 1), sub: change ? `${change} kg in 30 days` : null };
    }
  }
}

/** One small card of the "At a glance" grid. The whole card is a link to its detail page. */
export function GlanceCard({ id, today }: { id: HomeCard; today: Today }) {
  if (id === 'health_status') return <HealthStatusTile />;
  const b = build(id, today);
  return (
    <StatTile href={CARD_LINK[id]} data-card={id} label={CARD_LABEL[id]} value={b.value} {...(b.word ? { word: true } : {})} {...(b.sub ? { sub: b.sub } : {})} tone={b.alert ? 'warn' : 'muted'}>
      {b.extra}
    </StatTile>
  );
}

/**
 * Desktop: the "In Focus" grid shows these cards of the layout (when not hidden in Edit Home), next to
 * the two weekly activity cards that have no entry in the layout. "At a Glance" shows the rest.
 */
export const DESKTOP_FOCUS_CARDS: readonly HomeCard[] = ['energy', 'steps', 'sleep', 'training_status'];

/** Health status: how many of last night's overnight values are outside your usual range. */
function HealthStatusTile() {
  const q = useHealthStatus();
  const r = q.data;
  const word = r ? glanceWord(r) : null;
  return (
    <StatTile
      href={CARD_LINK.health_status}
      data-card="health_status"
      label={CARD_LABEL.health_status}
      value={q.isPending ? <Skeleton height={24} width="60%" /> : word ? <StatusWord type="health" value={word.state}>{word.text}</StatusWord> : null}
      word
      {...(r && r.has_night && r.compared > 0 ? { sub: `${r.compared} of ${r.metrics.length} values compared` } : {})}
      tone={r && r.outside > 0 ? 'warn' : 'muted'}
    >
      {r && r.has_night ? (
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
      ) : null}
    </StatTile>
  );
}
