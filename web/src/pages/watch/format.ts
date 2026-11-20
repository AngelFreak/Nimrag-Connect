import type { Maybe } from '../../lib/format';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

function plural(n: number, unit: string): string {
  return `${n} ${unit}${n === 1 ? '' : 's'} ago`;
}

/**
 * "5 min ago", "2 h ago", "3 days ago": how long ago something happened, in words a glance can read.
 * `null` for a missing or unreadable time. A time a little in the future (clock drift) counts as "just now".
 */
export function fmtAgo(iso: Maybe<string>, now: number = Date.now()): string | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return null;
  const diff = now - t;
  if (diff < MIN) return 'just now';
  if (diff < HOUR) return `${Math.floor(diff / MIN)} min ago`;
  if (diff < DAY) return `${Math.floor(diff / HOUR)} h ago`;
  const days = Math.floor(diff / DAY);
  if (days < 14) return plural(days, 'day');
  if (days < 60) return plural(Math.floor(days / 7), 'week');
  return plural(Math.floor(days / 30), 'month');
}

/** True when the time is missing or more than a day old: the watch has not been heard from lately. */
export function isStale(iso: Maybe<string>, now: number = Date.now()): boolean {
  if (!iso) return true;
  const t = Date.parse(iso);
  return Number.isNaN(t) || now - t > DAY;
}
