import { addDays } from '../../lib/dates';
import { fmtDurationMin, fmtInt, isMissing } from '../../lib/format';
import { fmtKm } from './movement';
import { localParts } from './time';
import type { ActivityListItem, CalendarDay } from './types';

export type Marker = 'cardio' | 'strength';

const KIND_WORD: Record<string, string> = {
  run: 'run',
  walk: 'walk',
  hike: 'hike',
  cycle: 'ride',
  swim: 'swim',
  strength: 'strength',
  training: 'training',
  other: 'activity',
};

export function kindWord(kind: string): string {
  return KIND_WORD[kind] ?? 'activity';
}

export function kindTitle(kind: string): string {
  const w = kindWord(kind);
  return w.charAt(0).toUpperCase() + w.slice(1);
}

/** Strength sessions get the square marker; everything else is cardio and gets the circle. */
export function markerOf(a: ActivityListItem): Marker {
  return a.kind === 'strength' ? 'strength' : 'cardio';
}

/** Which markers a day shows, cardio first, each at most once. */
export function dayMarkers(day: CalendarDay | undefined): Marker[] {
  if (!day) return [];
  const set = new Set(day.activities.map(markerOf));
  return (['cardio', 'strength'] as const).filter((m) => set.has(m));
}

/** "run, strength" or "run, 2 strength": what happened on the day, in words, for the cell's accessible name. */
export function activityWords(day: CalendarDay | undefined): string | null {
  if (!day || day.activities.length === 0) return null;
  const counts = new Map<string, number>();
  for (const a of day.activities) counts.set(kindWord(a.kind), (counts.get(kindWord(a.kind)) ?? 0) + 1);
  return [...counts].map(([w, n]) => (n > 1 ? `${n} ${w}s` : w)).join(', ');
}

/** Number of leading empty cells before day 1 (weeks start on Monday). */
export function leadingBlanks(month: string): number {
  return (new Date(`${month}-01T00:00:00Z`).getUTCDay() + 6) % 7;
}

/** All days of a "YYYY-MM" month. */
export function daysOfMonth(month: string): string[] {
  const first = new Date(`${month}-01T00:00:00Z`);
  const count = new Date(Date.UTC(first.getUTCFullYear(), first.getUTCMonth() + 1, 0)).getUTCDate();
  return Array.from({ length: count }, (_, i) => `${month}-${String(i + 1).padStart(2, '0')}`);
}

/** Monday = 0 .. Sunday = 6. */
export function weekdayIndex(day: string): number {
  return (new Date(`${day}T00:00:00Z`).getUTCDay() + 6) % 7;
}

export const CALENDAR_KEYS: readonly string[] = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown'];

/**
 * Where a key moves from `day`: arrows one day or one week, Home and End to the start and end
 * of the week row. Page Up and Page Down are handled by the caller (they change the month).
 */
export function moveFocus(day: string, key: string): string | null {
  switch (key) {
    case 'ArrowLeft':
      return addDays(day, -1);
    case 'ArrowRight':
      return addDays(day, 1);
    case 'ArrowUp':
      return addDays(day, -7);
    case 'ArrowDown':
      return addDays(day, 7);
    case 'Home':
      return addDays(day, -weekdayIndex(day));
    case 'End':
      return addDays(day, 6 - weekdayIndex(day));
    default:
      return null;
  }
}

/** "Sleep" value: "7h 12m · score 81", either half may be missing. */
export function sleepText(day: CalendarDay): string | null {
  const dur = isMissing(day.sleep_duration_s) ? null : fmtDurationMin(day.sleep_duration_s / 60);
  const score = fmtInt(day.sleep_score);
  if (dur === null && score === null) return null;
  return [dur, score === null ? null : `score ${score}`].filter(Boolean).join(' · ');
}

/** "High 94 · low 38", either half may be missing. */
export function energyText(day: CalendarDay): string | null {
  const hi = fmtInt(day.energy_high);
  const lo = fmtInt(day.energy_low);
  if (hi === null && lo === null) return null;
  return [hi === null ? null : `High ${hi}`, lo === null ? null : `low ${lo}`].filter(Boolean).join(' · ');
}

/** Row for an activity in the day summary: title "Run · 17:40", sub "38m · 6.4 km" or "42m · 18 sets". */
export function activityRow(a: ActivityListItem): { title: string; sub: string | null } {
  const t = localParts(a.start)?.time;
  const parts: (string | null)[] = [fmtDurationMin(isMissing(a.duration_s) ? null : a.duration_s / 60)];
  if (a.kind === 'strength') parts.push(isMissing(a.sets) ? null : `${a.sets} sets`);
  else parts.push(fmtKm(a.distance_m));
  const sub = parts.filter((p): p is string => p !== null).join(' · ');
  return { title: t ? `${kindTitle(a.kind)} · ${t}` : kindTitle(a.kind), sub: sub || null };
}
