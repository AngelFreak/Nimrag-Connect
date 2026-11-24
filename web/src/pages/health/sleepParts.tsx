import { useId, useState } from 'react';
import { ChevronDownIcon, Legend, Ring, StageTimeline, StatTile, type StageLine } from '../../components';
import type { NightSeries } from '../../api/generated/NightSeries';
import type { SleepNight } from '../../api/generated/SleepNight';
import { fmtFixed, fmtInt, withUnit } from '../../lib/format';
import { LEVEL_COLOR, LEVEL_NAME, stressLevelOf } from '../../lib/stress';
import { fmtSeconds, sleepTicks, stageSpans, stageSummary } from './model';
import { ChartSummary } from './parts';
import { RATING_COLOR, RATING_WORD, headlineFor, ratingOfScore, type Factor, type Rating } from './sleepNight';
import { clock } from './time';

export function RatingDot({ rating }: { rating: Rating }) {
  return <span className="rating-dot" aria-hidden="true" style={{ background: RATING_COLOR[rating] }} />;
}

/** The score ring with its level word, time asleep, bed and wake time. */
export function ScoreSummary({ night }: { night: SleepNight }) {
  const rating = ratingOfScore(night.score);
  const word = rating ? RATING_WORD[rating] : null;
  return (
    <div className="sleep-score">
      <div className="sleep-score__ring">
        <Ring value={night.score} label="Sleep score" color={rating ? RATING_COLOR[rating] : undefined} size={124} strokeWidth={11} />
        {word && rating ? (
          <span className="sleep-score__level">
            <RatingDot rating={rating} />
            {word}
          </span>
        ) : null}
      </div>
      <dl className="sleep-score__facts">
        <div>
          <dt>Time asleep</dt>
          <dd className="sleep-score__big">{fmtSeconds(night.duration_s) ?? <span className="unavailable">unavailable</span>}</dd>
        </div>
        <div>
          <dt>Bed</dt>
          <dd>{clock(night.start) ?? 'unavailable'}</dd>
        </div>
        <div>
          <dt>Woke</dt>
          <dd>{clock(night.end) ?? 'unavailable'}</dd>
        </div>
      </dl>
    </div>
  );
}

/** The headline and the one sentence under the ring. */
export function NightHeadline({ night, factors }: { night: SleepNight; factors: Factor[] }) {
  const h = headlineFor(night, factors);
  return (
    <div className="headline" data-rule={h.rule}>
      <p className="headline__title">{h.title}</p>
      <p className="headline__text">{h.sentence}</p>
    </div>
  );
}

/** The six factors; tap a row for its target band. */
export function FactorsList({ factors }: { factors: Factor[] }) {
  return (
    <ul className="sleep-factors" aria-label="Sleep score factors">
      {factors.map((f) => (
        <FactorItem key={f.key} factor={f} />
      ))}
    </ul>
  );
}

function FactorItem({ factor }: { factor: Factor }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <li className="sleep-factor" data-factor={factor.key} data-rating={factor.rating}>
      <button type="button" className="sleep-factor__head" aria-expanded={open} aria-controls={id} onClick={() => setOpen((o) => !o)}>
        <span className="sleep-factor__main">
          <span className="sleep-factor__name">{factor.name}</span>
          <span className="sleep-factor__value">{factor.value}</span>
        </span>
        <span className="sleep-factor__rating">
          <RatingDot rating={factor.rating} />
          {RATING_WORD[factor.rating]}
        </span>
        <span className="sleep-factor__chev" aria-hidden="true">
          <ChevronDownIcon size={16} />
        </span>
      </button>
      <p id={id} className="sleep-factor__target" hidden={!open}>
        {factor.target}
      </p>
    </li>
  );
}

// ---- stage chart with an overlay ---------------------------------------------------------------------

interface OverlayDef {
  key: string;
  label: string;
  color: string;
  decimals?: boolean;
}

const OVERLAYS: OverlayDef[] = [
  { key: 'heart_rate', label: 'Heart rate', color: 'var(--heart)' },
  { key: 'energy', label: 'Energy', color: 'var(--energy)' },
  { key: 'stress', label: 'Stress', color: 'var(--stress)' },
  { key: 'breathing', label: 'Breathing', color: 'var(--steps-dim)', decimals: true },
  { key: 'spo2', label: 'SpO2', color: 'var(--text)' },
  { key: 'hrv', label: 'HRV', color: 'var(--hrv-balanced)' },
];

/** The stage chart, the legend and chips that draw one more measurement over the night. */
export function StagesWithOverlay({ night, overlays }: { night: SleepNight; overlays: NightSeries[] }) {
  const [picked, setPicked] = useState<string | null>(null);
  const { spans, totalMinutes } = stageSpans(night);
  const available = OVERLAYS.filter((o) => (overlays.find((s) => s.key === o.key)?.points.length ?? 0) >= 2);
  const chosen = available.find((o) => o.key === picked) ?? null;
  const line: StageLine | undefined = chosen
    ? { name: chosen.label, color: chosen.color, decimals: chosen.decimals, points: overlays.find((s) => s.key === chosen.key)?.points ?? [] }
    : undefined;
  return (
    <>
      {night.stages_available && spans.length > 0 ? (
        <StageTimeline spans={spans} totalMinutes={totalMinutes} label="Sleep stages through the night" ticks={sleepTicks(night)} line={line} />
      ) : (
        <p className="stat__sub">Shows after your watch records sleep stages for a night.</p>
      )}
      {night.stages_available && spans.length > 0 ? <ChartSummary>{stageSummary(night)}</ChartSummary> : null}
      {night.stages_available && spans.length > 0 && available.length > 0 ? (
        <div role="group" aria-label="Draw over the stages" className="overlay-chips">
          {available.map((o) => (
            <button key={o.key} type="button" className="overlay-chip" aria-pressed={o.key === picked} onClick={() => setPicked(o.key === picked ? null : o.key)}>
              <span className="overlay-chip__dot" aria-hidden="true" style={{ background: o.color }} />
              {o.label}
            </button>
          ))}
        </div>
      ) : null}
      <Legend
        grid
        items={[
          { key: 'deep', label: 'Deep', color: 'var(--stage-deep)', value: fmtSeconds(night.deep_s), hasValue: true },
          { key: 'light', label: 'Light', color: 'var(--stage-light)', value: fmtSeconds(night.light_s), hasValue: true },
          { key: 'rem', label: 'REM', color: 'var(--stage-rem)', value: fmtSeconds(night.rem_s), hasValue: true },
          { key: 'awake', label: 'Awake', color: 'var(--stage-awake)', value: fmtSeconds(night.awake_s), hasValue: true },
        ]}
      />
    </>
  );
}

// ---- overnight strip ---------------------------------------------------------------------------------

/** "+50", "−12", "0": energy gained or lost over the night. */
export function signed(n: number): string {
  const r = Math.round(n);
  return r > 0 ? `+${r}` : r < 0 ? `−${Math.abs(r)}` : '0';
}

const HRV_STATUS_WORD: Record<string, string> = { balanced: 'Balanced', unbalanced: 'Unbalanced', low: 'Low', poor: 'Poor' };

/** The tiles of the night that have data; `null` when none has. */
export function overnightTiles(night: SleepNight): { key: string; label: string; value: string; sub?: string; subColor?: string }[] {
  const t: { key: string; label: string; value: string; sub?: string; subColor?: string }[] = [];
  const add = (key: string, label: string, value: string | null, sub?: string | null, subColor?: string) => {
    if (value !== null) t.push({ key, label, value, ...(sub ? { sub } : {}), ...(subColor ? { subColor } : {}) });
  };
  add('in_bed', 'Time in bed', fmtSeconds(night.time_in_bed_s));
  add('lowest_hr', 'Lowest heart rate', withUnit(fmtInt(night.lowest_hr_bpm), 'bpm'));
  add('avg_hr', 'Average heart rate', withUnit(fmtInt(night.avg_hr_bpm), 'bpm'));
  add('energy', 'Energy change', night.energy_change === null ? null : signed(night.energy_change));
  add('spo2', 'Blood oxygen', night.spo2_avg_percent === null ? null : `${fmtFixed(night.spo2_avg_percent, 1)} %`, night.spo2_low_percent === null ? null : `Lowest ${fmtFixed(night.spo2_low_percent, 0)} %`);
  add('breathing', 'Breathing rate', withUnit(fmtFixed(night.breathing_brpm, 1), '/min'), night.breathing_low_brpm === null ? null : `Lowest ${fmtFixed(night.breathing_low_brpm, 1)} /min`);
  add('hrv', 'Overnight HRV', withUnit(fmtInt(night.hrv_ms), 'ms'));
  add('hrv_status', '7-day HRV status', night.hrv_status === null ? null : (HRV_STATUS_WORD[night.hrv_status] ?? null));
  add('awakenings', 'Awakenings', fmtInt(night.awakenings));
  add('restless', 'Restless moments', fmtInt(night.restless_moments));
  if (night.avg_stress !== null) {
    const level = stressLevelOf(night.avg_stress);
    add('stress', 'Average sleep stress', fmtInt(night.avg_stress), LEVEL_NAME[level], LEVEL_COLOR[level]);
  }
  return t;
}

export function OvernightStrip({ night }: { night: SleepNight }) {
  const tiles = overnightTiles(night);
  if (tiles.length === 0) return null;
  return (
    <div className="overnight-strip" role="list" aria-label="Overnight measurements">
      {tiles.map((x) => (
        <div role="listitem" key={x.key} className="overnight-strip__cell">
          <StatTile
            size="sm"
            label={x.label}
            value={x.value}
            sub={x.sub ? (x.subColor ? <span className="overnight-strip__level"><span className="rating-dot" aria-hidden="true" style={{ background: x.subColor }} />{x.sub}</span> : x.sub) : undefined}
          />
        </div>
      ))}
    </div>
  );
}
