// Display helpers for the Today page. A missing number always gives `null`, which the
// components render as "unavailable" (never 0).

import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import type { BaselinePosition } from '../../api/generated/BaselinePosition';
import type { StressLevel } from '../../api/generated/StressLevel';
import { fmtDay } from '../../lib/dates';
import { fmtDurationMin, fmtFixed, fmtInt, isMissing, type Maybe } from '../../lib/format';

/** "07:42" in the owner's time zone; `null` for a missing or unreadable time. */
export function fmtTime(iso: Maybe<string>, timeZone: string): string | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return null;
  const opts: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' };
  try {
    return new Intl.DateTimeFormat('en-GB', { ...opts, timeZone }).format(new Date(t));
  } catch {
    return new Intl.DateTimeFormat('en-GB', opts).format(new Date(t));
  }
}

/** "Thursday 19 November" */
export function fmtLongDay(day: string, withYear = false): string {
  return fmtDay(day, { weekday: 'long', day: 'numeric', month: 'long', ...(withYear ? { year: 'numeric' } : {}) }).replace(',', '');
}

/** Seconds as "7h 12m". */
export function fmtSeconds(seconds: Maybe<number>): string | null {
  return isMissing(seconds) ? null : fmtDurationMin(seconds / 60);
}

export function unit(value: Maybe<number>, suffix: string, digits = 0): string | null {
  if (isMissing(value)) return null;
  const text = digits === 0 ? fmtInt(value) : fmtFixed(value, digits);
  return text === null ? null : `${text} ${suffix}`;
}

/** "+3.2" / "-0.5" for a change; `null` stays null. */
export function signed(value: Maybe<number>, digits = 0): string | null {
  if (isMissing(value)) return null;
  const rounded = Number(value.toFixed(digits));
  const body = Math.abs(rounded).toFixed(digits);
  return rounded > 0 ? `+${body}` : rounded < 0 ? `−${body}` : body;
}

export function energyHeadline(now: Maybe<number>): string | null {
  if (isMissing(now)) return null;
  if (now >= 60) return 'Good reserve';
  if (now >= 30) return 'Moderate reserve';
  return 'Low reserve';
}

const HRV_STATUS: Record<string, string> = { balanced: 'Balanced', unbalanced: 'Unbalanced', low: 'Low', poor: 'Poor' };

export function hrvStatusLabel(status: Maybe<string>): string | null {
  return status ? (HRV_STATUS[status] ?? null) : null;
}

export function baselineLabel(position: Maybe<BaselinePosition>): string | null {
  if (position === 'within') return 'Within your range';
  if (position === 'below') return 'Below your range';
  if (position === 'above') return 'Above your range';
  return null;
}

export function targetLabel(vs: Maybe<string>): string | null {
  if (vs === 'within') return 'Within target range';
  if (vs === 'below') return 'Below target range';
  if (vs === 'above') return 'Above target range';
  return null;
}

const STRESS_LEVEL: Record<StressLevel, string> = { rest: 'rest', low: 'low', medium: 'medium', high: 'high', activity: 'activity' };

export function stressLevelWord(level: Maybe<StressLevel>): string | null {
  return level ? STRESS_LEVEL[level] : null;
}

const KINDS: Record<string, string> = {
  run: 'Run',
  walk: 'Walk',
  hike: 'Hike',
  cycle: 'Cycle',
  swim: 'Swim',
  strength: 'Strength',
  training: 'Training',
  other: 'Other',
};

export function kindLabel(kind: string): string {
  return KINDS[kind] ?? kind.charAt(0).toUpperCase() + kind.slice(1);
}

/** "5:56 /km" */
export function fmtPace(secondsPerKm: Maybe<number>): string | null {
  if (isMissing(secondsPerKm) || secondsPerKm <= 0) return null;
  const total = Math.round(secondsPerKm);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')} /km`;
}

/** One line about an activity: strength gives sets and volume, others distance and pace. */
export function activityDetail(a: ActivityListItem): string | null {
  const parts: string[] = [];
  if (a.kind === 'strength') {
    if (!isMissing(a.sets)) parts.push(`${fmtInt(a.sets)} sets`);
    if (!isMissing(a.volume_kg)) parts.push(`${new Intl.NumberFormat('en-GB', { maximumFractionDigits: 1 }).format(a.volume_kg)} kg`);
  } else {
    if (!isMissing(a.distance_m)) parts.push(`${(a.distance_m / 1000).toFixed(2)} km`);
    const pace = fmtPace(a.avg_pace_s_per_km);
    if (pace) parts.push(pace);
  }
  return parts.length > 0 ? parts.join(' · ') : null;
}

export { activityTitle } from '../activities/format';

/** "19 Nov, 07:42" in the owner's time zone. */
export function fmtDateTime(iso: Maybe<string>, timeZone: string): string | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return null;
  const opts: Intl.DateTimeFormatOptions = { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' };
  try {
    return new Intl.DateTimeFormat('en-GB', { ...opts, timeZone }).format(new Date(t));
  } catch {
    return new Intl.DateTimeFormat('en-GB', opts).format(new Date(t));
  }
}

/** "212 MB" */
export function fmtBytes(bytes: Maybe<number>): string | null {
  if (isMissing(bytes)) return null;
  if (bytes < 1024) return `${fmtInt(bytes)} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

/** The top-bar status line: when the watch last reached the server. */
export function syncLine(lastSyncAt: Maybe<string>, lastDataAt: Maybe<string>, timeZone: string): string {
  const sync = fmtTime(lastSyncAt, timeZone);
  if (sync) return `Watch synced ${sync}`;
  const data = fmtTime(lastDataAt, timeZone);
  if (data) return `Latest watch data ${data}`;
  return 'No watch data yet';
}

/** "6.4 km" from metres; `null` stays null. */
export function fmtKm(meters: Maybe<number>, digits = 1): string | null {
  return isMissing(meters) ? null : `${(meters / 1000).toFixed(digits)} km`;
}

/** "Mon" to "Sun" letters for a day: "M", "T", "W" ... */
export function dayLetter(day: string): string {
  return fmtDay(day, { weekday: 'narrow' });
}
