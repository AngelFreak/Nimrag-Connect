import type { CSSProperties, ReactNode } from 'react';
import { Link } from 'react-router-dom';
import type { HomeCard, Today } from '../../api/generated';
import { Card, Skeleton, StatusWord, Unavailable, ValueOrUnavailable } from '../../components';
import { fmtShortDay } from '../../lib/dates';
import { fmtFixed, fmtInt, isMissing, statusName } from '../../lib/format';
import { LEVEL_NAME as STRESS_NAME } from '../../lib/stress';
import { statusColor } from '../../lib/status';
import { glanceWord, METRIC_SHORT, STATE_WORD } from '../health/healthStatus';
import { useHealthStatus } from '../health/healthStatusQueries';
import '../health/healthStatus.css';
import { LEVEL_COLOR, LEVEL_NAME } from '../health/readiness';
import { useReadiness } from '../health/readinessQueries';
import { RATING_WORD, ratingOfScore } from '../health/sleepNight';
import { CARD_LABEL, CARD_LINK } from './cards';
import { fmtSeconds, fmtTime, signed, unit } from './format';
import { Glyph, type GlyphName } from './glyphs';
import {
  HeartRateVisual,
  Hint,
  HrvVisual,
  IntensityVisual,
  LoadVisual,
  ProgressDial,
  ReadinessVisual,
  RecoveryVisual,
  Rows,
  SleepVisual,
  StatusDisc,
  StepsVisual,
  StressVisual,
  Vo2Visual,
  WeightChange,
  recoveryLeft,
} from './tileVisuals';

// One tile of the phone's "At a glance" grid: the card's glyph and name, a picture of fixed height, and
// one main figure with a short label. The whole tile is a link to the card's page.

/** Names that would take two lines in a half-width tile. */
const TILE_NAME: Partial<Record<HomeCard, string>> = { intensity_minutes: 'Intensity min' };

interface TileProps {
  id: HomeCard;
  glyph: GlyphName;
  /** Colour of the glyph. */
  tone: string;
  /** The picture, or the sentence that stands in for it. */
  visual: ReactNode;
  /** The main figure; null shows "unavailable", left out shows nothing (empty tiles). */
  value?: ReactNode;
  /** The figure is a word ("Productive") that may shrink to fit. */
  word?: boolean;
  label?: ReactNode;
  /** The label is a warning: something is out of range. */
  alert?: boolean;
  /** A line of its own under the label ("Nimrag estimate"). */
  note?: ReactNode;
  /** Replaces the value and its label (Sleep shows two figures). */
  figure?: ReactNode;
  /** Put the figure above the picture. */
  figureFirst?: boolean;
}

function Tile({ id, glyph, tone, visual, value, word = false, label, alert = false, note, figure, figureFirst = false }: TileProps) {
  return (
    <Link to={CARD_LINK[id]} className="tile-link" data-card={id}>
      <Card as="div" padding="sm" className={figureFirst ? 'tile tile--figure-first' : 'tile'}>
        <span className="tile__head" style={{ '--tone': tone } as CSSProperties}>
          <span className="tile__glyph">
            <Glyph name={glyph} size={18} />
          </span>
          <span className="tile__name">{TILE_NAME[id] ?? CARD_LABEL[id]}</span>
        </span>
        <div className="tile__visual">{visual}</div>
        <div className="tile__figure">
          {figure ?? (
            <>
              {/* No value at all (an empty tile): the hint in the picture says it, so no "unavailable" under it. */}
              {value === undefined ? null : (
                <span className={word ? 'tile__value tile__value--word' : 'tile__value'}>
                  <ValueOrUnavailable value={value} />
                </span>
              )}
              {label ? <span className={alert ? 'tile__label tone-warn' : 'tile__label'}>{label}</span> : null}
            </>
          )}
          {note ? <span className="tile__note">{note}</span> : null}
        </div>
      </Card>
    </Link>
  );
}

/** A tile while its own data loads: the same size as the finished one, so nothing moves. */
function TileLoading({ id, glyph, tone }: { id: HomeCard; glyph: GlyphName; tone: string }) {
  return <Tile id={id} glyph={glyph} tone={tone} visual={<Skeleton height={96} width="96px" />} value={<Skeleton height={24} width="60%" />} />;
}

// ---------------------------------------------------------------- the cards with their own data

function ReadinessTile() {
  const q = useReadiness();
  const r = q.data;
  const glyph = 'readiness';
  const tone = 'var(--zone-2)';
  if (q.isPending) return <TileLoading id="training_readiness" glyph={glyph} tone={tone} />;
  if (!r) return <Tile id="training_readiness" glyph={glyph} tone={tone} visual={<Hint>The estimate could not be loaded.</Hint>} />;
  if (r.score === null || !r.level) {
    return <Tile id="training_readiness" glyph={glyph} tone={tone} visual={<Hint>{r.unavailable ?? 'Not enough data for an estimate yet.'}</Hint>} note="Nimrag estimate" />;
  }
  return (
    <Tile
      id="training_readiness"
      glyph={glyph}
      tone={LEVEL_COLOR[r.level]}
      visual={<ReadinessVisual score={r.score} colors={LEVEL_COLOR} label={`Readiness estimate ${fmtInt(r.score)} out of 100, level ${LEVEL_NAME[r.level]}.`} />}
      value={
        <StatusWord type="readiness" value={r.level}>
          {LEVEL_NAME[r.level]}
        </StatusWord>
      }
      word
      label={r.headline}
      note="Nimrag estimate"
    />
  );
}

function HealthStatusTile() {
  const q = useHealthStatus();
  const r = q.data;
  const glyph = 'heart';
  const tone = 'var(--heart)';
  if (q.isPending) return <TileLoading id="health_status" glyph={glyph} tone={tone} />;
  if (!r) return <Tile id="health_status" glyph={glyph} tone={tone} visual={<Hint>Could not be loaded.</Hint>} />;
  if (!r.has_night) return <Tile id="health_status" glyph={glyph} tone={tone} visual={<Hint>No night recorded yet.</Hint>} />;
  const word = glanceWord(r);
  return (
    <Tile
      id="health_status"
      glyph={glyph}
      tone={tone}
      visual={
        <ul className="hs-list hs-list--tile" aria-label="Last night against your usual range">
          {r.metrics.map((m) => (
            <li key={m.key}>
              <span className="hs-list__name">{METRIC_SHORT[m.key]}</span>
              <StatusWord type="health" value={m.state}>
                {STATE_WORD[m.state]}
              </StatusWord>
            </li>
          ))}
        </ul>
      }
      value={
        <StatusWord type="health" value={word.state}>
          {word.text}
        </StatusWord>
      }
      word
      label={r.compared > 0 ? `${r.compared} of ${r.metrics.length} values compared` : null}
      alert={r.outside > 0}
    />
  );
}

// ---------------------------------------------------------------- the others, from the Today data

function Sleep({ today }: { today: Today }) {
  const s = today.sleep;
  const rating = s ? ratingOfScore(s.score) : null;
  return (
    <Tile
      id="sleep"
      glyph="sleep"
      tone="var(--sleep)"
      figureFirst={s !== null}
      visual={<SleepVisual today={today} />}
      figure={
        s ? (
          <div className="tile__pair">
            <span className="tile__pair-item">
              <span className="tile__value">{fmtInt(s.score) ?? <Unavailable />}</span>
              <span className="tile__label">
                Score
                {rating ? (
                  <span className="tile__rating">
                    <StatusWord type="sleep" value={rating}>
                      {RATING_WORD[rating]}
                    </StatusWord>
                  </span>
                ) : null}
              </span>
            </span>
            <span className="tile__pair-item">
              <span className="tile__value">{fmtSeconds(s.duration_s) ?? <Unavailable />}</span>
              <span className="tile__label">Asleep</span>
            </span>
          </div>
        ) : null
      }
    />
  );
}

function Tiles({ id, today }: { id: HomeCard; today: Today }) {
  const t = today;
  switch (id) {
    case 'heart_rate':
      // Nothing at all yet: the picture's sentence says so, no "unavailable" under it.
      if (isMissing(t.heart_rate.resting_hr_bpm) && t.heart_rate.day.series.length === 0) return <Tile id={id} glyph="heart" tone="var(--heart)" visual={<HeartRateVisual today={t} />} />;
      return <Tile id={id} glyph="heart" tone="var(--heart)" visual={<HeartRateVisual today={t} />} value={unit(t.heart_rate.resting_hr_bpm, 'bpm')} label="Resting" />;
    case 'stress': {
      const level = t.stress.main_level;
      if (isMissing(t.stress.average) && !level) return <Tile id={id} glyph="stress" tone="var(--stress)" visual={<StressVisual today={t} />} />;
      return (
        <Tile
          id={id}
          glyph="stress"
          tone="var(--stress)"
          visual={<StressVisual today={t} />}
          value={
            level ? (
              <StatusWord type="stress" value={level}>
                {STRESS_NAME[level]}
              </StatusWord>
            ) : null
          }
          word
          label="Most of today"
        />
      );
    }
    case 'sleep':
      return <Sleep today={t} />;
    case 'hrv':
      return <Tile id={id} glyph="hrv" tone="var(--heart)" visual={<HrvVisual today={t} />} value={unit(t.hrv.weekly_avg_ms, 'ms')} label="7-night average" />;
    case 'training_readiness':
      return <ReadinessTile />;
    case 'health_status':
      return <HealthStatusTile />;
    case 'training_status': {
      const s = t.training_status;
      const name = statusName(s.label);
      const since = s.since ? fmtShortDay(s.since.slice(0, 10)) : null;
      const color = statusColor('training', s.kind) ?? 'var(--status-none)';
      return (
        <Tile
          id={id}
          glyph="status"
          tone={color}
          visual={name ? <StatusDisc color={color} label={`Training status ${name}`} /> : <Hint>No training status yet.</Hint>}
          value={
            name ? (
              <StatusWord type="training" value={s.kind}>
                {name}
              </StatusWord>
            ) : null
          }
          word
          label={name && since ? `Since ${since}${s.since_is_lower_bound ? ' or earlier' : ''}` : null}
        />
      );
    }
    case 'steps':
      return <Tile id={id} glyph="steps" tone="var(--steps)" visual={<StepsVisual today={t} />} value={fmtInt(t.steps.steps)} label={`Goal ${fmtInt(t.steps.goal)}`} />;
    case 'intensity_minutes': {
      const m = t.intensity_minutes;
      // The ring holds this week's minutes; the figure says how far the goal still is.
      if (m.goal_min <= 0 || isMissing(m.total_min)) return <Tile id={id} glyph="intensity" tone="var(--warn)" visual={<IntensityVisual today={t} />} />;
      const left = m.goal_min - m.total_min;
      return <Tile id={id} glyph="intensity" tone="var(--warn)" visual={<IntensityVisual today={t} />} value={left > 0 ? `${fmtInt(left)} min` : 'Goal reached'} word={left <= 0} label={left > 0 ? `to go · goal ${fmtInt(m.goal_min)} this week` : `Goal ${fmtInt(m.goal_min)} this week`} />;
    }
    case 'training_load': {
      const l = t.training_load;
      const where = l.vs_target === 'within' ? 'In range' : l.vs_target === 'above' ? 'Above range' : l.vs_target === 'below' ? 'Below range' : null;
      return <Tile id={id} glyph="load" tone="var(--zone-1)" visual={<LoadVisual today={t} />} value={fmtInt(l.seven_day_load)} label={where ? `${where} · 7 days` : '7 days'} alert={l.vs_target === 'above' || l.vs_target === 'below'} />;
    }
    case 'recovery_time': {
      const r = t.recovery_time;
      const ready = fmtTime(r.ready_at, t.timezone);
      const done = r.remaining_s === 0;
      return (
        <Tile
          id={id}
          glyph="recovery"
          tone="var(--zone-2)"
          visual={recoveryLeft(r) === null ? <Hint>No recovery time from the watch yet.</Hint> : <RecoveryVisual today={t} />}
          value={done ? 'Recovered' : ready}
          word={done}
          label={done ? 'No recovery time left' : ready ? 'Ready around' : null}
        />
      );
    }
    case 'vo2_max': {
      const v = t.vo2max;
      const change = signed(v.change_3m, 1);
      return <Tile id={id} glyph="vo2" tone="var(--zone-4)" visual={<Vo2Visual today={t} />} value={fmtFixed(v.value, 1)} label={change ? `ml/kg/min · ${change} in 3 mo` : 'ml/kg/min'} />;
    }
    case 'respiration': {
      const r = t.respiration;
      const rows = [
        { name: 'Day average', value: fmtFixed(r.day_avg_brpm, 1) },
        { name: 'Asleep', value: fmtFixed(r.sleep_avg_brpm, 1) },
      ];
      return (
        <Tile
          id={id}
          glyph="breathing"
          tone="var(--zone-1)"
          visual={isMissing(r.latest_brpm) ? <Hint>No breathing rate yet.</Hint> : <Rows label="Breaths a minute" rows={rows.map((x) => ({ name: x.name, value: x.value ?? <Unavailable /> }))} />}
          value={fmtFixed(r.latest_brpm, 1)}
          label="Latest, breaths/min"
        />
      );
    }
    case 'spo2': {
      const o = t.spo2;
      const low = fmtInt(o.sleep_low_percent);
      return (
        <Tile
          id={id}
          glyph="spo2"
          tone="var(--zone-1)"
          visual={
            isMissing(o.latest_percent) ? (
              <Hint>No blood oxygen reading yet.</Hint>
            ) : (
              <ProgressDial label={`Blood oxygen ${fmtFixed(o.latest_percent, 0)} percent`} fraction={o.latest_percent / 100} color="var(--zone-1)" value={`${fmtFixed(o.latest_percent, 0)}%`} sub="latest" size={104} valueSize={24} />
            )
          }
          value={low ? `${low}%` : null}
          label={low ? 'Lowest asleep' : null}
        />
      );
    }
    case 'weight': {
      const w = t.weight;
      const change = signed(w.change_30d_kg, 1);
      return (
        <Tile
          id={id}
          glyph="weight"
          tone="var(--text-2)"
          visual={isMissing(w.weight_kg) ? <Hint>No weight logged yet.</Hint> : change && !isMissing(w.change_30d_kg) ? <WeightChange kg={w.change_30d_kg} text={change} /> : <Hint>Needs two weigh-ins to show a change.</Hint>}
          value={unit(w.weight_kg, 'kg', 1)}
          label="Latest"
        />
      );
    }
    case 'energy':
      return null;
  }
}

/** One tile of "At a glance". Energy has its own wide card, so it is never a tile. */
export function GlanceTile({ id, today }: { id: HomeCard; today: Today }) {
  return <Tiles id={id} today={today} />;
}
