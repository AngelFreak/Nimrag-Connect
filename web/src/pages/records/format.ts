// Display mapping for the personal records page. The server sends exact numbers; this module only
// chooses the unit and the words. A missing value is `null`, never 0.

import type { PersonalRecord } from '../../api/generated/PersonalRecord';
import { fmtDay } from '../../lib/dates';
import { fmtClock, fmtInt, isMissing, type Maybe } from '../../lib/format';
import { fmtDistance, fmtMeters } from '../activities/format';

interface Names {
  /** The record's row on the page. */
  label: string;
  /** The same in the list after "Personal record:". */
  short: string;
}

const NAMES: Record<string, Names> = {
  run_400m: { label: 'Fastest 400 m', short: '400 m' },
  run_1k: { label: 'Fastest 1 km', short: '1 km' },
  run_1mi: { label: 'Fastest 1 mile', short: '1 mile' },
  run_5k: { label: 'Fastest 5 km', short: '5 km' },
  run_10k: { label: 'Fastest 10 km', short: '10 km' },
  run_half: { label: 'Fastest half marathon', short: 'half marathon' },
  run_marathon: { label: 'Fastest marathon', short: 'marathon' },
  run_longest: { label: 'Longest run', short: 'longest run' },
  run_ascent: { label: 'Most ascent in a run', short: 'most ascent' },
  walk_longest: { label: 'Longest walk or hike', short: 'longest walk or hike' },
  walk_ascent: { label: 'Most ascent in a walk or hike', short: 'most ascent' },
  cycle_longest: { label: 'Longest ride', short: 'longest ride' },
  cycle_20k: { label: 'Fastest 20 km', short: '20 km' },
  cycle_40k: { label: 'Fastest 40 km', short: '40 km' },
  cycle_ascent: { label: 'Most ascent in a ride', short: 'most ascent' },
  longest_time: { label: 'Longest activity', short: 'longest activity' },
  steps_day: { label: 'Most steps in a day', short: 'most steps in a day' },
  steps_week: { label: 'Most steps in a week', short: 'most steps in a week' },
};

export function recordLabel(kind: string): string {
  return NAMES[kind]?.label ?? kind;
}

/** "Personal record: 5 km, 10 km" for the activity page; null when the activity holds none. */
export function badgeText(kinds: readonly string[]): string | null {
  if (kinds.length === 0) return null;
  const text = kinds.map((k) => NAMES[k]?.short ?? k).join(', ');
  return `${kinds.length === 1 ? 'Personal record' : 'Personal records'}: ${text}`;
}

export interface GroupText {
  title: string;
  empty: { title: string; body: string };
}

/** Section wording; the server sends the groups in the page's order. */
const GROUPS: Record<string, GroupText> = {
  run: { title: 'Running', empty: { title: 'No runs yet', body: 'Your fastest times and longest runs show up here after your first run.' } },
  walk: { title: 'Walking and hiking', empty: { title: 'No walks or hikes yet', body: 'Your longest walk and biggest climb show up here.' } },
  cycle: { title: 'Cycling', empty: { title: 'No rides yet', body: 'Your longest and fastest rides show up here.' } },
  all: { title: 'All sports', empty: { title: 'No activities yet', body: 'Sync the watch to bring in its activities.' } },
  steps: { title: 'Steps', empty: { title: 'No step counts yet', body: 'Daily and weekly step records show up once the watch has counted steps.' } },
};

export function groupText(key: string): GroupText {
  return GROUPS[key] ?? { title: key, empty: { title: 'Nothing yet', body: '' } };
}

/** The value as the page shows it: "25:03", "1:02:03", "10.00 km", "42 m", "12,345". Times are whole seconds, as the watch shows them. */
export function fmtRecordValue(r: Pick<PersonalRecord, 'kind' | 'unit' | 'value'>): string | null {
  if (isMissing(r.value)) return null;
  if (r.unit === 's') return fmtClock(r.value);
  if (r.unit === 'steps') return fmtInt(r.value);
  return r.kind.endsWith('_ascent') ? fmtMeters(r.value) : fmtDistance(r.value);
}

/** "Wed 11 Nov", with the year when it is not this year; a week record names its Monday: "Week of 2 Nov". */
export function fmtRecordDate(date: Maybe<string>, kind: string, today: string): string | null {
  if (!date) return null;
  const year = date.slice(0, 4) === today.slice(0, 4) ? {} : { year: 'numeric' as const };
  if (kind === 'steps_week') return `Week of ${fmtDay(date, { day: 'numeric', month: 'short', ...year })}`;
  return fmtDay(date, { weekday: 'short', day: 'numeric', month: 'short', ...year }).replace(',', '');
}

/** Whether any record of the group has a value; a group without one shows its empty state. */
export function groupHasRecords(records: readonly Pick<PersonalRecord, 'value'>[]): boolean {
  return records.some((r) => r.value !== null);
}
