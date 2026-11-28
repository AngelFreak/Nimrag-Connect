import { useCallback, useRef, useState, type KeyboardEvent } from 'react';
import { Link } from 'react-router-dom';
import type { HomeCard, HomeCards, Today } from '../../api/generated';
import type { ReadinessResponse } from '../../api/generated/ReadinessResponse';
import { Card, Legend, Ring, SeriesChart, StageBar, StatusWord, Unavailable } from '../../components';
import { energyKey } from '../../lib/status';
import { fmtInt, isMissing } from '../../lib/format';
import { LEVEL_COLOR, LEVEL_NAME } from '../health/readiness';
import { useReadiness } from '../health/readinessQueries';
import { RATING_COLOR, RATING_WORD, headlineFor, ratingOfScore } from '../health/sleepNight';
import { DEFAULT_HOME_CARDS } from './cards';
import { EnergyDayChart } from './EnergyDayChart';
import { baselineLabel, energyHeadline, fmtSeconds, fmtTime, hrvStatusLabel, signed, unit } from './format';

function EnergySlide({ today }: { today: Today }) {
  const e = today.energy;
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

/** Today's readiness estimate: the ring, its level word, the headline, and a link to the page that explains it. */
function ReadinessSlide({ data }: { data: ReadinessResponse }) {
  const level = data.level;
  const color = level ? LEVEL_COLOR[level] : 'var(--accent)';
  return (
    <Card as="div" padding="lg">
      <div className="focus-energy">
        <Ring value={data.score} label="Readiness estimate" color={color} />
        <div className="focus-energy__text">
          <span className="stat__label">Readiness</span>
          <span className="focus-energy__head">
            {level ? (
              <StatusWord type="readiness" value={level} size="lg">
                {LEVEL_NAME[level]}
              </StatusWord>
            ) : (
              <Unavailable />
            )}
          </span>
          {data.headline ? <span className="focus__headline">{data.headline}</span> : null}
          <span className="focus__sub">Nimrag estimate</span>
        </div>
      </div>
      {data.sentence ?? data.unavailable ? <p className="focus__sentence">{data.sentence ?? data.unavailable}</p> : null}
      <Link to="/training/readiness" className="focus__sub focus__link">
        What it is made from
      </Link>
    </Card>
  );
}

function SleepSlide({ today }: { today: Today }) {
  const s = today.sleep;
  const tz = today.timezone;
  if (!s) {
    return (
      <Card as="div" padding="lg">
        <span className="stat__label">Sleep last night</span>
        <p>
          <Unavailable />
        </p>
        <span className="focus__sub">The watch recorded no sleep for last night.</span>
      </Card>
    );
  }
  const bed = fmtTime(s.start, tz);
  const wake = fmtTime(s.end, tz);
  const rating = ratingOfScore(s.score);
  const headline = headlineFor(s);
  return (
    <Card as="div" padding="lg">
      <div className="focus-energy">
        <Ring value={s.score} label="Sleep score" color={rating ? RATING_COLOR[rating] : 'var(--sleep)'} />
        <div className="focus-energy__text">
          <span className="stat__label">Sleep last night</span>
          <span className="focus-energy__head">
            {rating ? (
              <StatusWord type="sleep" value={rating} size="lg">
                {RATING_WORD[rating]}
              </StatusWord>
            ) : (
              <Unavailable />
            )}
          </span>
          <span className="focus__asleep">{fmtSeconds(s.duration_s) ?? <Unavailable />} asleep</span>
          <span className="focus__sub">{bed && wake ? `${bed} to ${wake}` : null}</span>
        </div>
      </div>
      <p className="focus__headline">{headline.title}</p>
      {s.stages_available ? (
        <>
          <StageBar
            height={10}
            label={`Sleep stages: deep ${fmtSeconds(s.deep_s) ?? 'unavailable'}, light ${fmtSeconds(s.light_s) ?? 'unavailable'}, REM ${fmtSeconds(s.rem_s) ?? 'unavailable'}, awake ${fmtSeconds(s.awake_s) ?? 'unavailable'}`}
            segments={[
              { key: 'deep', label: 'Deep', value: s.deep_s, color: 'var(--stage-deep)' },
              { key: 'light', label: 'Light', value: s.light_s, color: 'var(--stage-light)' },
              { key: 'rem', label: 'REM', value: s.rem_s, color: 'var(--stage-rem)' },
              { key: 'awake', label: 'Awake', value: s.awake_s, color: 'var(--stage-awake)' },
            ]}
          />
          <Legend
            grid
            items={[
              { key: 'deep', label: 'Deep', color: 'var(--stage-deep)', value: fmtSeconds(s.deep_s), hasValue: true },
              { key: 'light', label: 'Light', color: 'var(--stage-light)', value: fmtSeconds(s.light_s), hasValue: true },
              { key: 'rem', label: 'REM', color: 'var(--stage-rem)', value: fmtSeconds(s.rem_s), hasValue: true },
              { key: 'awake', label: 'Awake', color: 'var(--stage-awake)', value: fmtSeconds(s.awake_s), hasValue: true },
            ]}
          />
        </>
      ) : (
        <span className="focus__sub">Sleep stages: <Unavailable /></span>
      )}
      <Link to="/sleep" className="focus__sub focus__link">
        Open last night
      </Link>
    </Card>
  );
}

function HrvSlide({ today }: { today: Today }) {
  const h = today.hrv;
  const values = h.values_4w.map((d) => d.value);
  const band: [number, number] | undefined =
    !isMissing(h.baseline_low_ms) && !isMissing(h.baseline_high_ms) ? [h.baseline_low_ms, h.baseline_high_ms] : undefined;
  const status = hrvStatusLabel(h.status);
  return (
    <Card as="div" padding="lg">
      <span className="stat__label">Overnight HRV</span>
      <div className="focus__row">
        <span className="focus__big">{unit(h.last_night_ms, 'ms') ?? <Unavailable />}</span>
        <div className="stat">
          <span className="stat__sub">
            {status ? <StatusWord type="hrv" value={h.status}>{status}</StatusWord> : null}
            {status && baselineLabel(h.vs_baseline) ? ' · ' : null}
            {baselineLabel(h.vs_baseline)}
          </span>
          <span className="focus__sub">{isMissing(h.weekly_avg_ms) ? null : `7-night average ${fmtInt(h.weekly_avg_ms)} ms`}</span>
        </div>
      </div>
      <SeriesChart
        label="Overnight HRV over the last 4 weeks"
        summary={`Overnight HRV over the last 4 weeks, ${values.filter((v) => v !== null).length} nights${band ? `, usual range ${band[0]} to ${band[1]} milliseconds` : ''}.`}
        x={values.map((_, i) => i)}
        series={[{ label: 'HRV', color: 'link', data: values }]}
        {...(band ? { band } : {})}
        height={72}
      />
      <span className="focus__sub">{band ? `Shaded band is your usual range, ${band[0]} to ${band[1]} ms.` : 'Your usual range is unavailable.'}</span>
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

  if (count === 0) return null;
  const current = Math.min(active, count - 1);
  return (
    <section className="focus" aria-roledescription="carousel" aria-label="In focus">
      <h2 className="section-label">In focus</h2>
      {/* The track scrolls sideways, so it must be reachable by keyboard (arrow keys move between slides). */}
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex, jsx-a11y/no-noninteractive-element-interactions */}
      <div ref={track} className="focus__track" data-scroll="x" tabIndex={0} aria-label="In focus cards, use the left and right arrow keys" role="group" onScroll={onScroll} onKeyDown={onKeyDown}>
        {slides.map((id, i) => (
          <div key={id} className="focus__slide" role="group" aria-roledescription="slide" aria-label={`${i + 1} of ${count}: ${SLIDE_NAME[id].title}`}>
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
