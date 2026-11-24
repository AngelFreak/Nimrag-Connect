// The sleep night explained: Nimrag's own rating model (built from public sleep guidelines, not
// from any vendor's), the six score factors, and the headline and sentence under the score.
// Pure functions; the page only draws what they return.

import type { SleepNight } from '../../api/generated/SleepNight';
import { fmtFixed } from '../../lib/format';
import { fmtSeconds } from './model';

export type Rating = 'excellent' | 'good' | 'fair' | 'poor';

export const RATING_WORD: Record<Rating, string> = { excellent: 'Excellent', good: 'Good', fair: 'Fair', poor: 'Poor' };
export const RATING_COLOR: Record<Rating, string> = {
  excellent: 'var(--rating-excellent)',
  good: 'var(--rating-good)',
  fair: 'var(--rating-fair)',
  poor: 'var(--rating-poor)',
};
/** Worst first, so a lower index is a weaker rating. */
const ORDER: Rating[] = ['poor', 'fair', 'good', 'excellent'];

/** The overall sleep score on its bands: 90 and up Excellent, 80 Good, 60 Fair, below Poor. */
export function ratingOfScore(score: number | null | undefined): Rating | null {
  if (score === null || score === undefined || Number.isNaN(score)) return null;
  return score >= 90 ? 'excellent' : score >= 80 ? 'good' : score >= 60 ? 'fair' : 'poor';
}

// ---- the rating rules -------------------------------------------------------------------------------

/** Time asleep against the need, both in minutes. More than 2 h over the need is Fair. */
export function rateDuration(asleepMin: number, needMin: number): Rating {
  if (asleepMin > needMin + 120) return 'fair';
  if (asleepMin >= needMin) return 'excellent';
  if (asleepMin >= needMin - 30) return 'good';
  if (asleepMin >= needMin - 90) return 'fair';
  return 'poor';
}

/** Average stress during sleep, 0 to 100. */
export function rateStress(avg: number): Rating {
  if (avg <= 15) return 'excellent';
  if (avg <= 20) return 'good';
  if (avg <= 30) return 'fair';
  return 'poor';
}

/** The share of time asleep (percent) against a target band: inside Excellent, then 4 and 8 points of slack. */
export function rateShare(percent: number, lo: number, hi: number): Rating {
  const away = percent < lo ? lo - percent : percent > hi ? percent - hi : 0;
  if (away === 0) return 'excellent';
  if (away <= 4) return 'good';
  if (away <= 8) return 'fair';
  return 'poor';
}

/** Minutes awake and the number of awakenings longer than 5 minutes. */
export function rateAwake(awakeMin: number, longAwakenings: number): Rating {
  if (awakeMin < 20 && longAwakenings <= 1) return 'excellent';
  if (awakeMin < 30) return 'good';
  if (awakeMin < 45) return 'fair';
  return 'poor';
}

export const SHARE_TARGET = { deep: [16, 33], light: [30, 64], rem: [21, 31] } as const;

/** Whole hours and minutes without padding: "8 h", "7 h 45 min". */
export function fmtHours(minutes: number): string {
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes - h * 60);
  if (h === 0) return `${m} min`;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

// ---- the factors ------------------------------------------------------------------------------------

export type FactorKey = 'duration' | 'stress' | 'deep' | 'light' | 'rem' | 'awake';

export interface Factor {
  key: FactorKey;
  name: string;
  /** The night's value as the page shows it ("6 h 40 min", "18.4 % · 1 h 12 min"). */
  value: string;
  rating: Rating;
  /** True when the rating comes from the watch's own sub-score. */
  fromWatch: boolean;
  /** The watch's own 0 to 100 sub-score when it rated this row. */
  watchScore: number | null;
  /** What the expanded row says: Nimrag's rule for its own rating; for a watch rating, how the watch scored it and then the target band. */
  target: string;
  /** Which side of the target band the value is on; `in` when inside or not a band. */
  side: 'low' | 'high' | 'in';
}

/** Awake stretches of more than 5 minutes; without stage rows, the watch's own count of awakenings. */
export function longAwakenings(night: SleepNight): number {
  if (night.stages_available && night.stages.length > 0) {
    return night.stages.filter((s) => s.stage === 'awake' && (Date.parse(s.end) - Date.parse(s.start)) / 60_000 > 5).length;
  }
  return night.awakenings ?? 0;
}

function pct(n: number): string {
  return `${fmtFixed(n, 1)} %`;
}

/** The lowest score of each rating on the 0 to 100 scale (the bands of `ratingOfScore`). */
const BAND_FROM: Record<Rating, number> = { poor: 0, fair: 60, good: 80, excellent: 90 };

/**
 * The line of a row the watch rated: its score, what the next level up needs, then the target band
 * for context. Nimrag's own rules ("within 4 points...") are not shown, because they did not rate it.
 */
export function watchLine(score: number, context: string): string {
  const rating = ratingOfScore(score) as Rating;
  const goal: Rating = rating === 'excellent' ? 'excellent' : (ORDER[ORDER.indexOf(rating) + 1] as Rating);
  return `Your watch scored this ${fmtFixed(score, 0)} of 100; ${RATING_WORD[goal]} is ${BAND_FROM[goal]} or more. ${context}`;
}

/** The six rows. A row with neither a value nor a watch score is left out. */
export function sleepFactors(night: SleepNight): Factor[] {
  const need = night.sleep_need_min;
  const sc = night.scores;
  const watch = (score: number | null): Rating | null => ratingOfScore(score);
  const out: Factor[] = [];

  // Duration
  {
    const min = night.duration_s / 60;
    const w = watch(sc.duration);
    out.push({
      key: 'duration',
      name: 'Duration',
      value: `${fmtSeconds(night.duration_s) ?? fmtHours(min)} of ${fmtHours(need)}`,
      rating: w ?? rateDuration(min, need),
      fromWatch: w !== null,
      watchScore: w === null ? null : sc.duration,
      target: w === null ? `Aim for your need of ${fmtHours(need)}. Within 30 min is Good, within 90 min is Fair.` : watchLine(sc.duration as number, `Your need is ${fmtHours(need)}.`),
      side: min < need ? 'low' : min > need + 120 ? 'high' : 'in',
    });
  }

  // Stress
  if (night.avg_stress !== null) {
    const w = watch(sc.recovery);
    out.push({
      key: 'stress',
      name: 'Stress',
      value: `${fmtFixed(night.avg_stress, 0)} average`,
      rating: w ?? rateStress(night.avg_stress),
      fromWatch: w !== null,
      watchScore: w === null ? null : sc.recovery,
      target: w === null ? 'Average stress while asleep: 15 or lower is Excellent, up to 20 Good, up to 30 Fair.' : watchLine(sc.recovery as number, 'It looks at how calm you were while asleep, so lower stress scores higher.'),
      side: 'in',
    });
  }

  // Stages as a share of the time asleep
  const asleep = (night.deep_s ?? 0) + (night.light_s ?? 0) + (night.rem_s ?? 0);
  const stageRows: { key: 'deep' | 'light' | 'rem'; name: string; secs: number | null; score: number | null }[] = [
    { key: 'deep', name: 'Deep sleep', secs: night.deep_s, score: sc.deep },
    { key: 'light', name: 'Light sleep', secs: night.light_s, score: sc.light },
    { key: 'rem', name: 'REM sleep', secs: night.rem_s, score: sc.rem },
  ];
  for (const r of stageRows) {
    const [lo, hi] = SHARE_TARGET[r.key];
    const w = watch(r.score);
    const share = r.secs !== null && asleep > 0 ? (r.secs / asleep) * 100 : null;
    if (share === null && w === null) continue;
    out.push({
      key: r.key,
      name: r.name,
      value: share === null ? 'No stage data' : `${fmtSeconds(r.secs)} · ${pct(share)}`,
      rating: w ?? rateShare(share ?? 0, lo, hi),
      fromWatch: w !== null,
      watchScore: w === null ? null : r.score,
      target: w === null ? `Target ${lo} to ${hi} % of your time asleep. Within 4 points is Good, within 8 Fair.` : watchLine(r.score as number, `Target ${lo} to ${hi} % of your time asleep.`),
      side: share === null ? 'in' : share < lo ? 'low' : share > hi ? 'high' : 'in',
    });
  }

  // Awake and restless
  {
    const w = watch(sc.quality);
    if (night.awake_s !== null || w !== null) {
      const awakeMin = (night.awake_s ?? 0) / 60;
      const long = longAwakenings(night);
      const parts: string[] = [];
      if (night.awake_s !== null) parts.push(`${fmtSeconds(night.awake_s)} awake`);
      if (night.awakenings !== null) parts.push(`${fmtFixed(night.awakenings, 0)} awakening${night.awakenings === 1 ? '' : 's'}`);
      if (night.restless_moments !== null) parts.push(`${fmtFixed(night.restless_moments, 0)} restless`);
      out.push({
        key: 'awake',
        name: 'Awake and restless',
        value: parts.join(' · ') || 'Not recorded',
        rating: w ?? rateAwake(awakeMin, long),
        fromWatch: w !== null,
        watchScore: w === null ? null : sc.quality,
        target: w === null ? 'Under 20 min awake and at most one wake-up over 5 min is Excellent. Under 30 min is Good, under 45 Fair.' : watchLine(sc.quality as number, 'Fewer and shorter wake-ups score higher.'),
        side: 'in',
      });
    }
  }
  return out.sort((a, b) => FACTOR_ORDER.indexOf(a.key) - FACTOR_ORDER.indexOf(b.key));
}

const FACTOR_ORDER: FactorKey[] = ['duration', 'stress', 'deep', 'light', 'rem', 'awake'];

/** Tie order when picking the weakest factor: awake first, then deep, REM, stress, light, duration. */
const TIE_ORDER: FactorKey[] = ['awake', 'deep', 'rem', 'stress', 'light', 'duration'];

export function weakestFactor(factors: Factor[]): Factor | null {
  let best: Factor | null = null;
  for (const key of TIE_ORDER) {
    const f = factors.find((x) => x.key === key);
    if (!f) continue;
    if (best === null || ORDER.indexOf(f.rating) < ORDER.indexOf(best.rating)) best = f;
  }
  return best;
}

// ---- headline and sentence ----------------------------------------------------------------------------

export type HeadlineCase =
  | 'short_night'
  | 'long_night'
  | 'restless'
  | 'light_shallow'
  | 'uneven_light'
  | 'little_deep'
  | 'lots_deep'
  | 'little_rem'
  | 'lots_rem'
  | 'tense'
  | 'a_bit_short'
  | 'solid';

export interface Headline {
  /** Which rule fired; for tests and the page's data attribute. */
  rule: HeadlineCase;
  title: string;
  sentence: string;
}

/**
 * One headline (2 to 6 words) and one sentence for the night. Rules, in order:
 * 1. more than 1 h short of the sleep need, or more than 1 h over it: a short or a long night;
 * 2. otherwise, unless every factor is Good or better, the weakest factor names the night;
 * 3. otherwise a positive headline.
 */
export function headlineFor(night: SleepNight, factors: Factor[] = sleepFactors(night)): Headline {
  const needMin = night.sleep_need_min;
  const diff = night.duration_s / 60 - needMin;
  const need = fmtHours(needMin);
  // The watch rated the time asleep Good or better: do not open with "Short night" over a Good row.
  const duration = factors.find((f) => f.key === 'duration');
  const watchOkShort = diff < -60 && duration !== undefined && duration.fromWatch && ORDER.indexOf(duration.rating) >= ORDER.indexOf('good');
  if (watchOkShort) {
    const rest = headlineFor({ ...night, duration_s: needMin * 60 }, factors);
    return { ...rest, sentence: `${rest.sentence} Time asleep was a bit shorter than your ${need} need.` };
  }
  if (diff < -60) {
    const gap = fmtHours(-diff);
    return diff < -180
      ? {
          rule: 'short_night',
          title: 'Very short night',
          sentence: `You slept ${gap} less than your ${need} need. An early, calm evening and a regular bedtime can help you catch up.`,
        }
      : {
          rule: 'short_night',
          title: 'Short night',
          sentence: `You slept ${gap} less than your ${need} need. Going to bed a little earlier tonight may help you catch up.`,
        };
  }
  if (diff > 60) {
    const gap = fmtHours(diff);
    return diff > 180
      ? {
          rule: 'long_night',
          title: 'Very long night',
          sentence: `You slept ${gap} more than your ${need} need. If this keeps happening, a steady wake-up time can keep your rhythm even.`,
        }
      : {
          rule: 'long_night',
          title: 'Long night',
          sentence: `You slept ${gap} more than your ${need} need. An occasional long night is normal, and a regular wake-up time keeps your rhythm steady.`,
        };
  }

  const weak = weakestFactor(factors);
  if (weak === null || ORDER.indexOf(weak.rating) >= ORDER.indexOf('good')) {
    return diff >= 0 && night.score !== null && night.score >= 90
      ? { rule: 'solid', title: 'Excellent, restful night', sentence: 'Your sleep ticked every box tonight. Keep the same bedtime and wind-down routine.' }
      : { rule: 'solid', title: 'Solid, restful night', sentence: 'Time asleep, stages and calm all looked healthy. A good base to build on.' };
  }
  const poor = weak.rating === 'poor';
  switch (weak.key) {
    case 'awake':
      return poor
        ? { rule: 'restless', title: 'Restless night', sentence: 'You were awake or restless for a good part of the night. A cooler, darker room and less caffeine late in the day may help.' }
        : { rule: 'restless', title: 'A few wake-ups', sentence: 'You were awake a little more than usual. Putting screens away an hour before bed may help you stay asleep.' };
    case 'deep':
      return weak.side === 'high'
        ? { rule: 'lots_deep', title: 'Lots of deep sleep', sentence: 'Deep sleep took a larger share than usual, which often follows a hard day or a short night before.' }
        : poor
          ? { rule: 'little_deep', title: 'Little deep sleep', sentence: 'Deep sleep was well below its usual share. Late hard training and caffeine late in the day can cut into it, so try finishing them earlier.' }
          : { rule: 'little_deep', title: 'Less deep sleep', sentence: 'Deep sleep was a bit low. A regular bedtime and a cool room often help it come back.' };
    case 'rem':
      return weak.side === 'high'
        ? { rule: 'lots_rem', title: 'Lots of REM sleep', sentence: 'REM took a larger share than usual. After a short night before, the body often makes up for it like this.' }
        : poor
          ? { rule: 'little_rem', title: 'Little REM sleep', sentence: 'REM sleep was well below its usual share. It builds up toward morning, so a cut-short night or a late alarm can trim it.' }
          : { rule: 'little_rem', title: 'Slightly low REM', sentence: 'REM sleep was a bit low. A steady wake-up time and a calmer evening can help.' };
    case 'stress':
      return poor
        ? { rule: 'tense', title: 'Tense night', sentence: 'Your stress stayed high while you slept. A short wind-down, such as slow breathing or a walk after dinner, may help tonight.' }
        : { rule: 'tense', title: 'A little tense', sentence: 'Your stress was somewhat raised during sleep. Winding down without screens before bed may help.' };
    case 'light':
      return weak.side === 'low'
        ? { rule: 'uneven_light', title: 'Uneven stages', sentence: 'Light sleep took a smaller share than usual, so the night leaned on deep and REM sleep. A regular bedtime helps keep the mix steady.' }
        : poor
          ? { rule: 'light_shallow', title: 'Light, shallow sleep', sentence: 'Most of the night was light sleep. A cool, dark, quiet room and no caffeine late in the day may help you sleep deeper.' }
          : { rule: 'light_shallow', title: 'On the light side', sentence: 'Light sleep took more of the night than usual. A regular bedtime and a cooler room can help.' };
    case 'duration':
    default:
      return { rule: 'a_bit_short', title: 'A little short', sentence: `You slept a bit under your ${need} need. A slightly earlier bedtime tonight can close the gap.` };
  }
}
