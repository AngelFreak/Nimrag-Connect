// Display mapping for activities. Every function returns `null` for a missing value so the
// components render "unavailable" (never 0).

import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import { fmtClock, fmtInt, isMissing, type Maybe } from '../../lib/format';
import { addDays, fmtDay, fmtMonth, fmtShortDay } from '../../lib/dates';

export const KIND_LABELS: Record<string, string> = {
  run: 'Run',
  walk: 'Walk',
  hike: 'Hike',
  cycle: 'Cycle',
  swim: 'Swim',
  strength: 'Strength',
  training: 'Training',
  other: 'Other',
};

/** Filter chips: the design's four plus the other kinds the API knows. */
export const TYPE_OPTIONS = [
  { value: 'all', label: 'All' },
  { value: 'run', label: 'Run' },
  { value: 'strength', label: 'Strength' },
  { value: 'walk', label: 'Walk' },
  { value: 'hike', label: 'Hike' },
  { value: 'cycle', label: 'Cycle' },
  { value: 'swim', label: 'Swim' },
  { value: 'training', label: 'Training' },
  { value: 'other', label: 'Other' },
] as const;
export type TypeKey = (typeof TYPE_OPTIONS)[number]['value'];

export function parseType(raw: string | null): TypeKey {
  return TYPE_OPTIONS.find((t) => t.value === raw)?.value ?? 'all';
}

/** Kinds that normally have a distance. */
export const DISTANCE_KINDS = new Set(['run', 'walk', 'hike', 'cycle', 'swim']);

function titleCase(s: string): string {
  const t = s.replace(/_/g, ' ').trim();
  return t ? t.charAt(0).toUpperCase() + t.slice(1) : t;
}

/** "Run", "Strength"; for training/other the watch's own sport name when it has one. */
export function activityTitle(a: { kind: string; sport_name: string | null }): string {
  if ((a.kind === 'other' || a.kind === 'training') && a.sport_name && a.sport_name !== 'generic') return titleCase(a.sport_name);
  return KIND_LABELS[a.kind] ?? titleCase(a.kind);
}

/** "42 min" under an hour, "1h 05m" above. */
export function fmtMinutes(seconds: Maybe<number>): string | null {
  if (isMissing(seconds)) return null;
  const total = Math.round(seconds / 60);
  if (total < 60) return `${total} min`;
  return `${Math.floor(total / 60)}h ${String(total % 60).padStart(2, '0')}m`;
}

/** "6.4 km"; "800 m" under a kilometre when `digits` is below 2; otherwise `digits` decimals. */
export function fmtDistance(meters: Maybe<number>, digits = 1): string | null {
  if (isMissing(meters)) return null;
  if (meters < 1000 && digits < 2) return `${Math.round(meters)} m`;
  return `${(meters / 1000).toFixed(digits)} km`;
}

/** "5:56 /km" from seconds per km. */
export function fmtPace(secPerKm: Maybe<number>): string | null {
  const c = fmtClock(secPerKm);
  return c === null ? null : `${c} /km`;
}

export function fmtSpeed(mps: Maybe<number>): string | null {
  return isMissing(mps) ? null : `${(mps * 3.6).toFixed(1)} km/h`;
}

export function fmtBpm(v: Maybe<number>): string | null {
  const n = fmtInt(v);
  return n === null ? null : `${n} bpm`;
}

export function fmtMeters(v: Maybe<number>): string | null {
  const n = fmtInt(v);
  return n === null ? null : `${n} m`;
}

export function fmtKcal(v: Maybe<number>): string | null {
  const n = fmtInt(v);
  return n === null ? null : `${n} kcal`;
}

export function fmtKg(v: Maybe<number>): string | null {
  if (isMissing(v)) return null;
  return `${new Intl.NumberFormat('en-GB', { maximumFractionDigits: 1 }).format(v)} kg`;
}

/** One decimal, "3.2"; null when missing. */
export function fmtOne(v: Maybe<number>): string | null {
  return isMissing(v) ? null : v.toFixed(1);
}

/** Local clock time of an owner-zone RFC 3339 string: "17:40". The server already applied the zone. */
export function clockOf(start: string | null | undefined): string | null {
  const m = start ? /T(\d{2}:\d{2})/.exec(start) : null;
  return m?.[1] ?? null;
}

/** "Mon 16 Nov · 17:40" */
export function whenLabel(a: { date: string; start: string }): string {
  const clock = clockOf(a.start);
  return clock ? `${fmtShortDay(a.date)} · ${clock}` : fmtShortDay(a.date);
}

export interface RowText {
  /** Right-hand figure of a phone row. */
  value: string | null;
  /** Line under it. */
  sub: string | null;
  /** Desktop "Detail" column. */
  detail: string | null;
}

function join(parts: (string | null)[]): string | null {
  const p = parts.filter((x): x is string => x !== null);
  return p.length ? p.join(' · ') : null;
}

/** The design's row text: strength "42 min / 18 sets · 3,300 kg", run "6.4 km / 38:00 · 5:56 /km", walk "4.8 km / 52 min". */
export function rowText(a: ActivityListItem): RowText {
  const duration = fmtMinutes(a.duration_s);
  if (a.kind === 'strength') {
    const sets = a.sets === null ? null : `${a.sets} ${a.sets === 1 ? 'set' : 'sets'}`;
    const volume = a.volume_kg === null ? (a.sets === null ? null : 'volume unavailable') : fmtKg(Math.round(a.volume_kg));
    const detail = join([sets, volume]);
    return { value: duration, sub: detail, detail };
  }
  const distance = fmtDistance(a.distance_m);
  if (distance !== null) {
    const pace = a.kind === 'run' ? fmtPace(a.avg_pace_s_per_km) : null;
    const sub = a.kind === 'run' ? join([fmtClock(a.duration_s), pace]) : duration;
    return { value: distance, sub, detail: join([distance, pace]) };
  }
  const hr = a.avg_hr_bpm === null ? null : `avg ${fmtBpm(a.avg_hr_bpm)}`;
  return { value: duration, sub: hr ?? fmtKcal(a.calories_kcal), detail: null };
}

// ---- grouping by week and month -------------------------------------------------------------

/** Monday of the week that holds `day` (all arithmetic in UTC on calendar days). */
export function weekStart(day: string): string {
  const dow = (new Date(`${day}T00:00:00Z`).getUTCDay() + 6) % 7; // Monday = 0
  return addDays(day, -dow);
}

/** "This week", "Last week", otherwise the month ("October 2026"). */
export function groupLabel(day: string, today: string): string {
  const thisWeek = weekStart(today);
  const w = weekStart(day);
  if (w >= thisWeek) return 'This week';
  if (w === addDays(thisWeek, -7)) return 'Last week';
  return fmtMonth(day.slice(0, 7));
}

export interface Group<T> {
  label: string;
  items: T[];
}

/** Consecutive groups of a newest-first list. */
export function groupByPeriod<T extends { date: string }>(items: readonly T[], today: string): Group<T>[] {
  const out: Group<T>[] = [];
  for (const it of items) {
    const label = groupLabel(it.date, today);
    const last = out[out.length - 1];
    if (last && last.label === label) last.items.push(it);
    else out.push({ label, items: [it] });
  }
  return out;
}

// ---- sorting (desktop table) -----------------------------------------------------------------

export type SortKey = 'date' | 'type' | 'duration' | 'hr';
export interface Sort {
  key: SortKey;
  dir: 'asc' | 'desc';
}

function sortValue(a: ActivityListItem, key: SortKey): string | number | null {
  switch (key) {
    case 'date':
      return a.start;
    case 'type':
      return activityTitle(a);
    case 'duration':
      return a.duration_s;
    case 'hr':
      return a.avg_hr_bpm;
  }
}

/** Missing values always sort last, whichever the direction. */
export function sortActivities(items: readonly ActivityListItem[], sort: Sort): ActivityListItem[] {
  const sign = sort.dir === 'asc' ? 1 : -1;
  return [...items].sort((a, b) => {
    const x = sortValue(a, sort.key);
    const y = sortValue(b, sort.key);
    if (x === null && y === null) return 0;
    if (x === null) return 1;
    if (y === null) return -1;
    if (typeof x === 'string' && typeof y === 'string') return x.localeCompare(y) * sign;
    return ((x as number) - (y as number)) * sign;
  });
}

// ---- desktop list rows and detail headline ------------------------------------------------------

export interface ListStat {
  label: string;
  /** null means the watch did not record it ("unavailable"). */
  value: string | null;
}

/** The five figures of a desktop list row, chosen by sport from what the list API returns. */
export function listStats(a: ActivityListItem): ListStat[] {
  const time = fmtClock(a.duration_s);
  const distance = fmtDistance(a.distance_m, 2);
  const ascent = fmtMeters(a.ascent_m);
  const avgHr = fmtBpm(a.avg_hr_bpm);
  switch (a.kind) {
    case 'run':
    case 'walk':
    case 'hike':
      return [
        { label: 'Distance', value: distance },
        { label: 'Time', value: time },
        { label: 'Avg pace', value: fmtPace(a.avg_pace_s_per_km) },
        { label: 'Total ascent', value: ascent },
        { label: 'Avg HR', value: avgHr },
      ];
    case 'cycle':
      return [
        { label: 'Distance', value: distance },
        { label: 'Time', value: time },
        { label: 'Avg speed', value: a.distance_m && a.duration_s ? fmtSpeed(a.distance_m / a.duration_s) : null },
        { label: 'Total ascent', value: ascent },
        { label: 'Avg HR', value: avgHr },
      ];
    case 'swim':
      return [
        { label: 'Distance', value: fmtDistance(a.distance_m, 1) },
        { label: 'Time', value: time },
        { label: 'Avg HR', value: avgHr },
        { label: 'Max HR', value: fmtBpm(a.max_hr_bpm) },
        { label: 'Calories', value: fmtKcal(a.calories_kcal) },
      ];
    case 'strength':
      return [
        { label: 'Time', value: time },
        { label: 'Exercises', value: a.exercises === null ? null : String(a.exercises) },
        { label: 'Sets', value: a.sets === null ? null : String(a.sets) },
        { label: 'Volume', value: a.volume_kg === null ? null : fmtKg(Math.round(a.volume_kg)) },
        { label: 'Avg HR', value: avgHr },
      ];
    default:
      return [
        { label: 'Time', value: time },
        { label: 'Distance', value: distance },
        { label: 'Avg HR', value: avgHr },
        { label: 'Max HR', value: fmtBpm(a.max_hr_bpm) },
        { label: 'Calories', value: fmtKcal(a.calories_kcal) },
      ];
  }
}

/** "Monday · 18:14", the line under a row's name. */
export function weekdayClock(a: { date: string; start: string }): string {
  const day = fmtDay(a.date, { weekday: 'long' });
  const clock = clockOf(a.start);
  return clock ? `${day} · ${clock}` : day;
}

/** "November 8, 2026 @ 17:22", the breadcrumb of an activity. */
export function longWhen(a: { date: string; start: string }): string {
  const day = new Intl.DateTimeFormat('en-US', { timeZone: 'UTC', month: 'long', day: 'numeric', year: 'numeric' }).format(new Date(`${a.date}T00:00:00Z`));
  const clock = clockOf(a.start);
  return clock ? `${day} @ ${clock}` : day;
}

/** The sort choices of the desktop list: a label and the Sort it stands for. */
export const SORT_CHOICES: readonly { id: string; label: string; sort: Sort }[] = [
  { id: 'newest', label: 'Newest first', sort: { key: 'date', dir: 'desc' } },
  { id: 'oldest', label: 'Oldest first', sort: { key: 'date', dir: 'asc' } },
  { id: 'longest', label: 'Longest first', sort: { key: 'duration', dir: 'desc' } },
  { id: 'hr', label: 'Highest avg HR', sort: { key: 'hr', dir: 'desc' } },
  { id: 'type', label: 'Type A to Z', sort: { key: 'type', dir: 'asc' } },
];

/** The id of the choice a Sort matches; the default when it is one of the table's other states. */
export function sortChoiceId(sort: Sort): string {
  return SORT_CHOICES.find((c) => c.sort.key === sort.key && c.sort.dir === sort.dir)?.id ?? 'newest';
}

/** Rows whose type name or sport name contains the text (case-insensitive); everything when it is empty. */
export function searchActivities<T extends { kind: string; sport_name: string | null; sub_sport_name?: string | null }>(items: readonly T[], text: string): T[] {
  const q = text.trim().toLowerCase();
  if (!q) return [...items];
  return items.filter((a) =>
    [activityTitle(a), a.kind, a.sport_name, a.sub_sport_name ?? null].some((s) => s !== null && s.replace(/_/g, ' ').toLowerCase().includes(q)),
  );
}
