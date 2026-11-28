import { useCallback, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from 'react';
import { Link } from 'react-router-dom';
import type { HomeCard, HomeCards, Today } from '../../api/generated';
import type { ReadinessFactor } from '../../api/generated/ReadinessFactor';
import type { ReadinessRating } from '../../api/generated/ReadinessRating';
import type { ReadinessResponse } from '../../api/generated/ReadinessResponse';
import { Card, Legend, Ring, SeriesChart, StatusWord, Unavailable } from '../../components';
import { energyKey } from '../../lib/status';
import { fmtMonthDay } from '../../lib/dates';
import { fmtFixed, fmtInt, isMissing } from '../../lib/format';
import { FACTOR_META, LEVEL_COLOR, LEVEL_NAME, RATING_COLOR as FACTOR_COLOR, factorValue, factorWord } from '../health/readiness';
import { useReadiness } from '../health/readinessQueries';
import { RATING_COLOR, RATING_WORD, ratingOfScore } from '../health/sleepNight';
import { DEFAULT_HOME_CARDS } from './cards';
import { EnergyDayChart } from './EnergyDayChart';
import { Glyph, type GlyphName } from './glyphs';
import { HrvVisual, ReadinessVisual, SleepVisual } from './tileVisuals';
import { baselineLabel, energyHeadline, fmtSeconds, fmtTime, signed, unit } from './format';

/** A slide without data: its name, one sentence of why, and where to look instead. No "unavailable" lines. */
function EmptySlide({ glyph, title, children, to, linkText }: { glyph: GlyphName; title: string; children: string; to: string; linkText: string }) {
  return (
    <Card as="div" padding="lg" className="focus-empty">
      <span className="focus-empty__glyph" aria-hidden="true">
        <Glyph name={glyph} size={28} />
      </span>
      <span className="focus-empty__title">{title}</span>
      <p className="focus-empty__text">{children}</p>
      <Link to={to} className="focus__sub focus__link">
        {linkText}
      </Link>
    </Card>
  );
}

function EnergySlide({ today }: { today: Today }) {
  const e = today.energy;
  if (isMissing(e.now) && e.series.length === 0) {
    return (
      <EmptySlide glyph="energy" title="Energy" to="/metrics/energy" linkText="See earlier days">
        No energy readings from the watch yet today.
      </EmptySlide>
    );
  }
  const headline = energyHeadline(e.now);
  return (
    <Card as="div" padding="lg">
      <div className="focus-energy">
        <Ring value={e.now} label="Energy level" color="var(--energy)" />
        <div className="focus-energy__text">
          <span className="stat__label">Energy</span>
          <span className="focus-energy__head">{headline ? <StatusWord type="energy" value={energyKey(e.now)} size="lg">{headline}</StatusWord> : <Unavailable />}</span>
          <div className="focus-energy__flows">
            <span className="tone-energy">{isMissing(e.charged) ? <>Charged <Unavailable /></> : `${signed(e.charged)} charged`}</span>
            <span className="tone-warn">{isMissing(e.drained) ? <>Drained <Unavailable /></> : `${signed(-e.drained)} drained`}</span>
            {fmtTime(e.wake_time, today.timezone) ? (
              <span className="focus-energy__woke">
                <i className="day-chart__wake" aria-hidden="true" />
                Woke {fmtTime(e.wake_time, today.timezone)}
              </span>
            ) : null}
          </div>
        </div>
      </div>
      <EnergyDayChart energy={e} stress={today.stress} timezone={today.timezone} />
    </Card>
  );
}

/** A ring without a score: just the empty track and a dash, no "unavailable". */
function EmptyRing({ label }: { label: string }) {
  return (
    <svg className="ring" width={104} height={104} viewBox="0 0 104 104" role="img" aria-label={`${label}: none yet`}>
      <circle cx={52} cy={52} r={45} fill="none" style={{ stroke: 'var(--track)' }} strokeWidth={10} />
      <text x={52} y={52 + 104 * 0.08} textAnchor="middle" fontFamily="var(--font-display)" fontWeight={600} fontSize={104 * 0.29} style={{ fill: 'var(--text-2)' }}>
        –
      </text>
    </svg>
  );
}

/** The fill of a factor's bar: its rating's rung out of five. */
const RUNG: Record<ReadinessRating, number> = { very_poor: 1, poor: 2, moderate: 3, good: 4, very_good: 5 };

/** The six factors, heaviest first: name, value and word, and a short bar in the rating's colour. */
function FactorBars({ factors }: { factors: ReadinessFactor[] }) {
  return (
    <ul className="focus-factors" aria-label="What the estimate is made from">
      {factors.map((f) => {
        const value = factorValue(f);
        const rating = value === null ? null : f.rating;
        return (
          <li key={f.key} className="focus-factors__row" aria-label={`${FACTOR_META[f.key].name}: ${rating ? `${value}, ${factorWord(f.key, rating)}` : 'no data'}`}>
            <span className="focus-factors__name" aria-hidden="true">
              {FACTOR_META[f.key].name}
            </span>
            <span className={rating ? 'focus-factors__value' : 'focus-factors__value focus-factors__value--none'} aria-hidden="true">
              {rating ? <span className="focus-factors__figure">{value}</span> : '–'}
            </span>
            <span className="focus-factors__track" aria-hidden="true">
              {rating ? <i style={{ width: `${(RUNG[rating] / 5) * 100}%`, background: FACTOR_COLOR[rating] }} /> : null}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

/** Today's readiness estimate: the gauge, its level word and headline, the six factors, and a link to the page that explains it. */
function ReadinessSlide({ data }: { data: ReadinessResponse }) {
  const level = data.level;
  return (
    <Card as="div" padding="lg">
      <div className="focus-energy">
        {data.score !== null ? (
          <ReadinessVisual score={data.score} colors={LEVEL_COLOR} label={`Readiness estimate ${fmtInt(data.score)} out of 100${level ? `, level ${LEVEL_NAME[level]}` : ''}.`} />
        ) : (
          <EmptyRing label="Readiness estimate" />
        )}
        <div className="focus-energy__text">
          <span className="stat__label">Readiness</span>
          <span className="focus-energy__head">
            {level ? (
              <StatusWord type="readiness" value={level} size="lg">
                {LEVEL_NAME[level]}
              </StatusWord>
            ) : (
              <span className="focus__sub">No estimate yet</span>
            )}
          </span>
          {data.headline ? <span className="focus__headline">{data.headline}</span> : null}
          <span className="focus__sub">Nimrag estimate</span>
        </div>
      </div>
      {data.score === null && data.unavailable ? <p className="focus__sentence">{data.unavailable}</p> : null}
      <FactorBars factors={data.factors} />
    </Card>
  );
}

/** One small figure of the overnight facts row. */
function Fact({ label, value, unitText }: { label: string; value: string; unitText: string }) {
  return (
    <div className="focus-fact">
      <span className="focus-fact__label">{label}</span>
      <span className="focus-fact__value">
        {value}
        <span className="focus-fact__unit"> {unitText}</span>
      </span>
    </div>
  );
}

function SleepSlide({ today }: { today: Today }) {
  const s = today.sleep;
  const tz = today.timezone;
  if (!s) {
    return (
      <EmptySlide glyph="sleep" title="Sleep last night" to="/sleep" linkText="See your latest nights">
        No sleep from last night has reached Nimrag. It shows here once the watch has synced the night.
      </EmptySlide>
    );
  }
  const bed = fmtTime(s.start, tz);
  const wake = fmtTime(s.end, tz);
  const rating = ratingOfScore(s.score);
  const asleep = fmtSeconds(s.duration_s);
  const lowHr = s.lowest_hr_bpm ?? today.heart_rate.resting_hr_bpm;
  const hrv = s.hrv_ms ?? today.hrv.last_night_ms;
  const breathing = today.respiration.sleep_avg_brpm ?? s.breathing_brpm;
  const facts: ReactNode[] = [
    isMissing(lowHr) ? null : <Fact key="hr" label={isMissing(s.lowest_hr_bpm) ? 'Resting heart rate' : 'Lowest heart rate'} value={fmtInt(lowHr) ?? ''} unitText="bpm" />,
    isMissing(hrv) ? null : <Fact key="hrv" label="Overnight HRV" value={fmtInt(hrv) ?? ''} unitText="ms" />,
    isMissing(breathing) ? null : <Fact key="br" label="Breathing" value={fmtFixed(breathing, 1) ?? ''} unitText="per min" />,
    isMissing(s.energy_change) ? null : <Fact key="en" label="Energy overnight" value={signed(s.energy_change) ?? ''} unitText="points" />,
  ].filter((x) => x !== null);
  const stages = [
    { key: 'deep', label: 'Deep', color: 'var(--stage-deep)', value: fmtSeconds(s.deep_s), hasValue: true },
    { key: 'light', label: 'Light', color: 'var(--stage-light)', value: fmtSeconds(s.light_s), hasValue: true },
    { key: 'rem', label: 'REM', color: 'var(--stage-rem)', value: fmtSeconds(s.rem_s), hasValue: true },
    { key: 'awake', label: 'Awake', color: 'var(--stage-awake)', value: fmtSeconds(s.awake_s), hasValue: true },
  ].filter((i) => i.value !== null);
  return (
    <Card as="div" padding="lg">
      <div className="focus-energy">
        {isMissing(s.score) ? <EmptyRing label="Sleep score" /> : <Ring value={s.score} label="Sleep score" color={rating ? RATING_COLOR[rating] : 'var(--sleep)'} />}
        <div className="focus-energy__text">
          <span className="stat__label">Sleep last night</span>
          <span className="focus-energy__head">
            {rating ? (
              <StatusWord type="sleep" value={rating} size="lg">
                {RATING_WORD[rating]}
              </StatusWord>
            ) : (
              <span className="focus__sub">No score for this night</span>
            )}
          </span>
          {asleep ? <span className="focus__asleep">{asleep} asleep</span> : null}
          {bed && wake ? <span className="focus__sub">{`${bed} to ${wake}`}</span> : null}
        </div>
      </div>
      {s.stages_available ? (
        <div className="focus__body">
          <div className="focus__visual focus__fill">
            <SleepVisual today={today} height={96} />
          </div>
          <Legend items={stages} />
        </div>
      ) : (
        <div className="focus__body">
          <p className="focus__sub">No sleep stages for this night.</p>
          {facts.length > 0 ? (
            <div className="focus-facts" style={{ '--cols': Math.min(3, facts.length) } as CSSProperties}>
              {facts.slice(0, 3)}
            </div>
          ) : null}
        </div>
      )}
    </Card>
  );
}

function HrvSlide({ today }: { today: Today }) {
  const h = today.hrv;
  const days = h.values_4w;
  const values = days.map((d) => d.value);
  if (isMissing(h.last_night_ms) && values.every((v) => v === null)) {
    return (
      <EmptySlide glyph="hrv" title="Overnight HRV" to="/hrv" linkText="See HRV status">
        No overnight HRV yet. Wear the watch while you sleep and it shows here in the morning.
      </EmptySlide>
    );
  }
  const band: [number, number] | undefined =
    !isMissing(h.baseline_low_ms) && !isMissing(h.baseline_high_ms) ? [h.baseline_low_ms, h.baseline_high_ms] : undefined;
  const versus = baselineLabel(h.vs_baseline);
  const big = unit(isMissing(h.last_night_ms) ? h.weekly_avg_ms : h.last_night_ms, 'ms');
  const known = values.filter((v): v is number => v !== null);
  const labelAt = (i: number) => ({ value: i, text: fmtMonthDay(days[i]!.date) });
  const last = days.length - 1;
  const xLabels = days.length >= 4 ? [labelAt(0), labelAt(Math.round(last / 3)), labelAt(Math.round((last * 2) / 3)), labelAt(last)] : [];
  return (
    <Card as="div" padding="lg">
      <div className="focus__row">
        {big ? <span className="focus__big">{big}</span> : null}
        <div className="stat">
          <span className="stat__label">Overnight HRV</span>
          <span className="focus__sub">{isMissing(h.last_night_ms) ? 'No reading last night' : isMissing(h.weekly_avg_ms) ? 'Last night' : `Last night · 7-night average ${fmtInt(h.weekly_avg_ms)} ms`}</span>
          {band ? <span className="focus__sub">{versus ? `${versus} (${band[0]}–${band[1]} ms)` : `Usual range ${band[0]}–${band[1]} ms`}</span> : versus ? <span className="focus__sub">{versus}</span> : null}
        </div>
      </div>
      <div className="focus__body">
        <div className="focus__visual">
          <HrvVisual today={today} />
        </div>
        {known.length > 0 ? (
          <SeriesChart
            label="Overnight HRV over the last 4 weeks"
            summary={`Overnight HRV over the last 4 weeks, ${known.length} nights, between ${fmtInt(Math.min(...known))} and ${fmtInt(Math.max(...known))} milliseconds${band ? `, usual range ${band[0]} to ${band[1]} milliseconds` : ''}.`}
            x={values.map((_, i) => i)}
            series={[{ label: 'HRV', color: 'link', data: values, points: known.length <= 6 }]}
            {...(band ? { band } : {})}
            xRange={[0, Math.max(1, last)]}
            {...(xLabels.length ? { xLabels } : {})}
            height={80}
          />
        ) : null}
      </div>
    </Card>
  );
}

type SlideId = 'energy' | 'readiness' | 'sleep' | 'hrv';

/** The home card each slide belongs to, so Edit home can order and hide the slides too. */
const SLIDE_CARD: Record<SlideId, HomeCard> = { energy: 'energy', readiness: 'training_readiness', sleep: 'sleep', hrv: 'hrv' };
const SLIDE_NAME: Record<SlideId, { dot: string; title: string }> = {
  energy: { dot: 'Energy', title: 'Energy' },
  readiness: { dot: 'Readiness', title: 'Readiness' },
  sleep: { dot: 'Sleep', title: 'Sleep last night' },
  hrv: { dot: 'HRV', title: 'Overnight HRV' },
};

/** The slides to show, in the owner's card order, without the hidden cards (and without readiness until its answer is in). */
export function focusSlides(cards: HomeCards, hasReadiness: boolean): SlideId[] {
  const hidden = new Set(cards.hidden);
  return (Object.keys(SLIDE_CARD) as SlideId[])
    .filter((id) => !hidden.has(SLIDE_CARD[id]) && (id !== 'readiness' || hasReadiness))
    .sort((a, b) => cards.order.indexOf(SLIDE_CARD[a]) - cards.order.indexOf(SLIDE_CARD[b]));
}

function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * "In focus": a swipeable row of large cards (scroll-snap) with page dots.
 * The track takes focus and answers the left and right arrow keys; the dots are buttons.
 */
export function FocusCarousel({ today, cards = DEFAULT_HOME_CARDS }: { today: Today; cards?: HomeCards }) {
  const track = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(0);
  const readiness = useReadiness();
  // The readiness slide shows whenever the API answered, with its own sentence when there is no estimate yet.
  const estimate = readiness.data ?? null;
  const slides = focusSlides(cards, estimate !== null);
  const count = slides.length;

  const goTo = useCallback(
    (index: number) => {
      const el = track.current;
      if (!el) return;
      const i = Math.max(0, Math.min(count - 1, index));
      const slide = el.children[i] as HTMLElement | undefined;
      if (!slide) return;
      // Position of the slide's left edge inside the track's scroll area.
      const left = slide.offsetLeft - (el.firstElementChild as HTMLElement).offsetLeft;
      if (typeof el.scrollTo === 'function') el.scrollTo({ left, behavior: prefersReducedMotion() ? 'auto' : 'smooth' });
      else el.scrollLeft = left;
      setActive(i);
    },
    [count],
  );

  const onScroll = useCallback(() => {
    const el = track.current;
    if (!el) return;
    const first = el.firstElementChild as HTMLElement | null;
    const second = el.children[1] as HTMLElement | undefined;
    if (!first || !second) return;
    const step = second.offsetLeft - first.offsetLeft;
    if (step <= 0) return;
    const atEnd = el.scrollLeft + el.clientWidth >= el.scrollWidth - 2;
    setActive(atEnd ? count - 1 : Math.max(0, Math.min(count - 1, Math.round(el.scrollLeft / step))));
  }, [count]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.target !== e.currentTarget) return;
    if (e.key === 'ArrowRight') {
      e.preventDefault();
      goTo(active + 1);
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      goTo(active - 1);
    } else if (e.key === 'Home') {
      e.preventDefault();
      goTo(0);
    } else if (e.key === 'End') {
      e.preventDefault();
      goTo(count - 1);
    }
  };

  const current = Math.min(active, Math.max(0, count - 1));
  // Every slide is exactly as tall as the Energy slide (the first slide when Energy is hidden):
  // that slide keeps its natural height and the others are set to it.
  // Without energy readings the Energy slide is a short note: then every slide stretches to the
  // tallest instead, so nothing is cut off.
  const energyEmpty = isMissing(today.energy.now) && today.energy.series.length === 0;
  const ref = energyEmpty ? -1 : Math.max(0, slides.indexOf('energy'));
  const [height, setHeight] = useState<number | undefined>(undefined);
  useLayoutEffect(() => {
    const slide = ref < 0 ? undefined : (track.current?.children[ref] as HTMLElement | undefined);
    if (!slide) {
      setHeight(undefined);
      return;
    }
    const measure = () => {
      const h = slide.offsetHeight;
      if (h > 0) setHeight(h);
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(measure);
    ro.observe(slide);
    return () => ro.disconnect();
  }, [ref, count]);

  if (count === 0) return null;
  return (
    <section className="focus" aria-roledescription="carousel" aria-label="In focus">
      <h2 className="section-label">In focus</h2>
      {/* The track scrolls sideways, so it must be reachable by keyboard (arrow keys move between slides). */}
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex, jsx-a11y/no-noninteractive-element-interactions */}
      <div ref={track} className={ref < 0 ? 'focus__track focus__track--stretch' : 'focus__track'} data-scroll="x" style={height === undefined ? undefined : ({ '--focus-h': `${height}px` } as CSSProperties)} tabIndex={0} aria-label="In focus cards, use the left and right arrow keys" role="group" onScroll={onScroll} onKeyDown={onKeyDown}>
        {slides.map((id, i) => (
          <div key={id} className={i === ref ? 'focus__slide focus__slide--ref' : 'focus__slide'} role="group" aria-roledescription="slide" aria-label={`${i + 1} of ${count}: ${SLIDE_NAME[id].title}`}>
            {id === 'energy' ? <EnergySlide today={today} /> : id === 'readiness' && estimate ? <ReadinessSlide data={estimate} /> : id === 'sleep' ? <SleepSlide today={today} /> : <HrvSlide today={today} />}
          </div>
        ))}
      </div>
      <div className="focus__dots" role="group" aria-label="Choose a card">
        {slides.map((id, i) => (
          <button key={id} type="button" className="focus__dot" aria-label={`Show ${SLIDE_NAME[id].dot}, card ${i + 1} of ${count}`} aria-current={current === i ? 'true' : undefined} onClick={() => goTo(i)} />
        ))}
      </div>
    </section>
  );
}
