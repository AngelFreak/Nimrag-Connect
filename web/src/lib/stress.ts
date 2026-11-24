// Stress levels, in one place: the thresholds the server uses (server/src/web_api/daily.rs), their
// colours, the time-in-level shares and the plain sentence under the day chart. Pure functions.

import type { StressBlock } from '../api/generated/StressBlock';
import type { StressLevel } from '../api/generated/StressLevel';
import type { StressSample } from '../api/generated/StressSample';

/** The four measured levels, calmest first. Activity (movement, no value) is not a level of stress. */
export const MEASURED_LEVELS = ['rest', 'low', 'medium', 'high'] as const;
export type MeasuredLevel = (typeof MEASURED_LEVELS)[number];

export const LEVEL_NAME: Record<StressLevel, string> = { rest: 'Rest', low: 'Low', medium: 'Medium', high: 'High', activity: 'Activity' };

/** The value range of each level, as the legend shows it. */
export const LEVEL_RANGE: Record<MeasuredLevel, string> = { rest: '0–25', low: '26–50', medium: '51–75', high: '76–100' };

export const LEVEL_COLOR: Record<StressLevel, string> = {
  rest: 'var(--stress-rest)',
  low: 'var(--stress-low)',
  medium: 'var(--stress-medium)',
  high: 'var(--stress-high)',
  activity: 'var(--stress-activity)',
};

/** Level of a 0 to 100 stress value: 0-25 rest, 26-50 low, 51-75 medium, 76-100 high. */
export function stressLevelOf(value: number): MeasuredLevel {
  if (value <= 25) return 'rest';
  if (value <= 50) return 'low';
  if (value <= 75) return 'medium';
  return 'high';
}

/** Seconds per level of one day (or the mean per day of a period). `null` is "not reported". */
export type LevelSeconds = Record<StressLevel, number | null>;

export function levelSeconds(b: Pick<StressBlock, 'rest_s' | 'low_s' | 'medium_s' | 'high_s' | 'activity_s'>): LevelSeconds {
  return { rest: b.rest_s, low: b.low_s, medium: b.medium_s, high: b.high_s, activity: b.activity_s };
}

export interface LevelShare {
  level: StressLevel;
  seconds: number;
  /** Whole minutes, as the page shows them. */
  minutes: number;
  /** Whole percent of the total; the shares add up to exactly 100 (largest remainder). */
  percent: number;
}

/** The levels that have time, calmest first, with minutes and percent. Empty when there is no time at all. */
export function levelShares(s: LevelSeconds): LevelShare[] {
  const all: StressLevel[] = [...MEASURED_LEVELS, 'activity'];
  const present = all.flatMap((level) => {
    const seconds = s[level];
    return seconds !== null && Number.isFinite(seconds) && seconds > 0 ? [{ level, seconds }] : [];
  });
  const total = present.reduce((a, p) => a + p.seconds, 0);
  if (total <= 0) return [];
  const raw = present.map((p) => (p.seconds / total) * 100);
  const percents = raw.map(Math.floor);
  let left = 100 - percents.reduce((a, b) => a + b, 0);
  const order = raw.map((r, i) => ({ i, frac: r - Math.floor(r) })).sort((a, b) => b.frac - a.frac || a.i - b.i);
  for (const { i } of order) {
    if (left <= 0) break;
    percents[i] = (percents[i] as number) + 1;
    left -= 1;
  }
  return present.map((p, i) => ({ level: p.level, seconds: p.seconds, minutes: Math.round(p.seconds / 60), percent: percents[i] as number }));
}

// ---- the sentence under the day chart ---------------------------------------------------------------

/** Less than this of medium or high stress is described as "little". */
const MIN_STRETCH_S = 10 * 60;

function minutesWords(seconds: number): string {
  const total = Math.max(1, Math.round(seconds / 60));
  const h = Math.floor(total / 60);
  const m = total % 60;
  const hours = h === 0 ? '' : `${h} ${h === 1 ? 'hour' : 'hours'}`;
  const mins = m === 0 ? '' : `${m} ${m === 1 ? 'minute' : 'minutes'}`;
  return [hours, mins].filter(Boolean).join(' ');
}

const DOMINANT_WORDS: Record<MeasuredLevel, string> = {
  rest: 'Mostly resting',
  low: 'Mostly low stress',
  medium: 'Mostly medium stress',
  high: 'Mostly high stress',
};

/** The measured level with the most time; on a tie the calmer one. `null` without any measured time. */
export function dominantLevel(s: LevelSeconds): MeasuredLevel | null {
  let best: MeasuredLevel | null = null;
  let top = 0;
  for (const level of MEASURED_LEVELS) {
    const v = s[level] ?? 0;
    if (v > top) {
      top = v;
      best = level;
    }
  }
  return best;
}

const pad = (n: number) => String(n).padStart(2, '0');

/**
 * The two-hour window (on whole hours) holding most of the medium and high samples, when it holds more
 * than half of them: 14:00 to 16:00. Clock hours are read from the sample text, which is written in the
 * owner's zone already.
 */
export function elevatedWindow(samples: StressSample[]): { from: string; to: string } | null {
  const hours = new Array<number>(24).fill(0);
  let n = 0;
  for (const s of samples) {
    if (s.level !== 'medium' && s.level !== 'high') continue;
    const h = Number(s.time.slice(11, 13));
    if (s.time[10] !== 'T' || !Number.isInteger(h) || h < 0 || h > 23) continue;
    hours[h] = (hours[h] as number) + 1;
    n += 1;
  }
  if (n === 0) return null;
  let start = 0;
  let best = -1;
  // Windows 00-02 ... 22-24; the first one wins a tie.
  for (let h = 0; h <= 22; h += 1) {
    const sum = (hours[h] as number) + (hours[h + 1] as number);
    if (sum > best) {
      best = sum;
      start = h;
    }
  }
  if (best * 2 <= n) return null;
  return { from: `${pad(start)}:00`, to: `${pad(start + 2)}:00` };
}

/**
 * One plain line about the day from its numbers: the level that lasted longest, then how much medium
 * and high stress there was and, when it was bunched up, when. No advice. `null` without any reading.
 */
export function daySentence(b: Pick<StressBlock, 'rest_s' | 'low_s' | 'medium_s' | 'high_s' | 'activity_s' | 'series'>): string | null {
  const secs = levelSeconds(b);
  const main = dominantLevel(secs);
  if (main === null) return null;
  const parts: string[] = [];
  if ((secs.high ?? 0) >= MIN_STRETCH_S) parts.push(`${minutesWords(secs.high as number)} of high`);
  if ((secs.medium ?? 0) >= MIN_STRETCH_S) parts.push(`${minutesWords(secs.medium as number)} of medium`);
  if (parts.length === 0) return `${DOMINANT_WORDS[main]}; little medium or high stress.`;
  const win = elevatedWindow(b.series);
  const where = win ? `, most of it between ${win.from} and ${win.to}` : '';
  // "40 minutes of medium stress"; "20 minutes of high and 40 minutes of medium stress".
  const tail = parts.length === 2 ? `${parts[0]} and ${parts[1]} stress` : `${parts[0]} stress`;
  return `${DOMINANT_WORDS[main]}; ${tail}${where}.`;
}
