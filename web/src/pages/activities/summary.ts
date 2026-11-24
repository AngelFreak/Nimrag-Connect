// What the top of an activity page shows for each sport, and how the rest is grouped into Stats.

import type { ActivityDetail } from '../../api/generated';
import { fmtClock, fmtInt, isMissing, type Maybe } from '../../lib/format';
import type { Headline, StatGroup } from './DetailParts';
import { fmtBpm, fmtDistance, fmtKcal, fmtKg, fmtMeters, fmtPace, fmtSpeed } from './format';
import { spread } from './series';

const PACED = new Set(['run', 'walk', 'hike']);

/** "Treadmill" or "Trail": the watch's sub sport, when it says more than the sport does. */
export function subSportText(a: Pick<ActivityDetail, 'sub_sport_name'>): string | null {
  const s = a.sub_sport_name?.replace(/_/g, ' ').trim();
  if (!s || s === 'generic') return null;
  return s.charAt(0).toUpperCase() + s.slice(1);
}

export function isPaced(kind: string): boolean {
  return PACED.has(kind);
}

/** "1,240" for a count of reps. */
function fmtCount(v: Maybe<number>): string | null {
  return isMissing(v) ? null : new Intl.NumberFormat('en-GB', { maximumFractionDigits: 1 }).format(v);
}

/** The four or five key figures of an activity, by sport. A figure without a value is left out when drawn. */
export function headlineFor(a: ActivityDetail): Headline[] {
  const time: Headline = { id: 'time', label: 'Time', value: fmtClock(a.duration_s) };
  const distance: Headline = { id: 'distance', label: 'Distance', value: fmtDistance(a.distance_m) };
  const avgHr: Headline = { id: 'hr', label: 'Avg HR', value: fmtBpm(a.avg_hr_bpm) };
  const ascent: Headline = { id: 'ascent', label: 'Ascent', value: fmtMeters(a.ascent_m) };
  const energy: Headline = { id: 'energy', label: 'Energy', value: fmtKcal(a.calories_kcal) };
  switch (a.kind) {
    case 'run':
    case 'walk':
    case 'hike':
      return [distance, time, { id: 'pace', label: 'Avg pace', value: fmtPace(a.avg_pace_s_per_km) }, avgHr, ascent];
    case 'cycle':
      return [distance, time, { id: 'speed', label: 'Avg speed', value: fmtSpeed(a.avg_speed_mps) }, avgHr, ascent];
    case 'swim':
      return [distance, time, avgHr, energy];
    case 'strength':
      return [time, { id: 'sets', label: 'Sets', value: a.sets === null ? null : String(a.sets) }, { id: 'reps', label: 'Reps', value: fmtCount(a.reps) }, avgHr];
    default:
      return [time, avgHr, energy];
  }
}

/** The figures with a value, as shown. */
export function shownHeadline(a: ActivityDetail): (Headline & { value: string })[] {
  return headlineFor(a).filter((h): h is Headline & { value: string } => h.value !== null && h.value !== '');
}

/**
 * The Stats: the figures the top of the page does not already show, in groups. Groups with nothing
 * in them are dropped.
 */
export function statGroupsFor(a: ActivityDetail): StatGroup[] {
  const alt = a.series ? spread(a.series.altitude_m) : null;
  const cad = a.series ? spread(a.series.cadence) : null;
  const paced = isPaced(a.kind);
  const elapsedDiffers = a.elapsed_s !== null && a.duration_s !== null && Math.abs(a.elapsed_s - a.duration_s) >= 1;
  const shown = new Set(headlineFor(a).filter((h) => h.value !== null && h.value !== '').map((h) => h.id));
  const groups: StatGroup[] = [
    { title: 'Distance', items: [{ id: 'distance', label: 'Distance', value: fmtDistance(a.distance_m) }] },
    {
      title: 'Timing',
      items: [
        { id: 'time', label: 'Time', value: fmtClock(a.duration_s) },
        { label: 'Elapsed time', value: elapsedDiffers ? fmtClock(a.elapsed_s) : null },
      ],
    },
    {
      title: paced ? 'Pace' : 'Speed',
      items: [
        { id: 'pace', label: 'Avg pace', value: paced ? fmtPace(a.avg_pace_s_per_km) : null },
        { id: 'speed', label: 'Avg speed', value: fmtSpeed(a.avg_speed_mps) },
        { label: 'Max speed', value: fmtSpeed(a.max_speed_mps) },
      ],
    },
    {
      title: 'Heart rate',
      items: [
        { id: 'hr', label: 'Avg HR', value: fmtBpm(a.avg_hr_bpm) },
        { label: 'Max HR', value: fmtBpm(a.max_hr_bpm) },
      ],
    },
    {
      title: 'Elevation',
      items: [
        { id: 'ascent', label: 'Ascent', value: fmtMeters(a.ascent_m) },
        { label: 'Descent', value: fmtMeters(a.descent_m) },
        { label: 'Lowest point', value: alt ? fmtMeters(alt.min) : null },
        { label: 'Highest point', value: alt ? fmtMeters(alt.max) : null },
      ],
    },
    {
      title: paced ? 'Running dynamics' : 'Cadence',
      items: [
        { label: 'Avg cadence', value: a.avg_cadence === null ? null : `${fmtInt(a.avg_cadence)} /min` },
        { label: 'Max cadence', value: cad ? `${fmtInt(cad.max)} /min` : null },
      ],
    },
    {
      title: 'Strength',
      items: [
        { id: 'sets', label: 'Sets', value: a.sets === null ? null : String(a.sets) },
        { id: 'reps', label: 'Reps', value: fmtCount(a.reps) },
        { label: 'Exercises', value: a.exercises === null ? null : String(a.exercises) },
        { label: 'Volume', value: a.volume_kg === null ? null : fmtKg(Math.round(a.volume_kg)) },
      ],
    },
    { title: 'Energy', items: [{ id: 'energy', label: 'Energy', value: fmtKcal(a.calories_kcal) }] },
  ];
  return groups
    .map((g) => ({ ...g, items: g.items.filter((i) => i.value !== null && i.value !== '' && !(i.id !== undefined && shown.has(i.id))) }))
    .filter((g) => g.items.length > 0);
}
