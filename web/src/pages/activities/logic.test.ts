import { describe, expect, it } from 'vitest';
import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import type { ExerciseGroup } from '../../api/generated/ExerciseGroup';
import type { StrengthSet } from '../../api/generated/StrengthSet';
import {
  activityTitle,
  groupByPeriod,
  groupLabel,
  listStats,
  longWhen,
  parseType,
  rowText,
  searchActivities,
  sortActivities,
  sortChoiceId,
  SORT_CHOICES,
  weekdayClock,
  weekStart,
  whenLabel,
} from './format';
import { distanceAxis, distanceLabels, minutesAxis, paceMinutes, spread, timeLabels, timedToMinutes, timeTicks } from './series';
import { buildPatch, compareExercises, deltaText, differsFromWatch, draftsOf, loadText, parseReps, parseWeight, repsText, validateAll } from './strengthModel';
import { zoneEntries } from './ActivityBody';

function item(over: Partial<ActivityListItem> = {}): ActivityListItem {
  return {
    id: 'a'.repeat(64),
    kind: 'run',
    sport_name: 'running',
    sub_sport_name: 'generic',
    start: '2026-11-16T17:40:00+00:00',
    date: '2026-11-16',
    duration_s: 2280,
    distance_m: 6400,
    avg_pace_s_per_km: 356,
    avg_hr_bpm: 151,
    max_hr_bpm: 170,
    calories_kcal: 468,
    ascent_m: 42,
    sets: null,
    volume_kg: null,
    exercises: null,
    ...over,
  };
}

function set(idx: number, over: Partial<StrengthSet> = {}): StrengthSet {
  return {
    idx,
    reps: 10,
    weight_kg: 22,
    duration_s: null,
    exercise: 'Bench press',
    edited: false,
    original_reps: 10,
    original_weight_kg: 22,
    original_exercise: 'Bench press',
    ...over,
  };
}

const group = (sets: StrengthSet[], name = 'Bench press'): ExerciseGroup => ({ name, sets, volume_kg: null });

describe('row text', () => {
  it('matches the design for run, strength and walk', () => {
    expect(rowText(item())).toEqual({ value: '6.40 km', sub: '38:00 · 5:56 /km', detail: '6.40 km · 5:56 /km' });
    expect(rowText(item({ kind: 'strength', distance_m: null, avg_pace_s_per_km: null, duration_s: 2520, sets: 18, volume_kg: 3300 }))).toEqual({
      value: '42:00',
      sub: '18 sets · 3,300 kg',
      detail: '18 sets · 3,300 kg',
    });
    expect(rowText(item({ kind: 'walk', duration_s: 3120, distance_m: 4800, avg_pace_s_per_km: 650 })).sub).toBe('52:00');
  });

  it('never turns a missing value into 0', () => {
    const t = rowText(item({ distance_m: null, duration_s: null, avg_pace_s_per_km: null, avg_hr_bpm: null, calories_kcal: null, kind: 'other' }));
    expect(t).toEqual({ value: null, sub: null, detail: null });
    const s = rowText(item({ kind: 'strength', sets: 6, volume_kg: null, duration_s: null }));
    expect(s.value).toBeNull();
    expect(s.sub).toBe('6 sets · volume unavailable');
  });

  it('titles and date labels', () => {
    expect(activityTitle(item())).toBe('Run');
    expect(activityTitle(item({ kind: 'other', sport_name: 'paddle_boarding' }))).toBe('Paddle boarding');
    expect(whenLabel(item())).toBe('Mon 16 Nov · 17:40');
    expect(parseType('strength')).toBe('strength');
    expect(parseType('nonsense')).toBe('all');
    expect(parseType(null)).toBe('all');
  });
});

describe('grouping and sorting', () => {
  it('groups into this week, last week, then months', () => {
    expect(weekStart('2026-11-19')).toBe('2026-11-16');
    expect(groupLabel('2026-11-18', '2026-11-19')).toBe('This week');
    expect(groupLabel('2026-11-15', '2026-11-19')).toBe('Last week');
    expect(groupLabel('2026-11-09', '2026-11-19')).toBe('Last week');
    expect(groupLabel('2026-11-08', '2026-11-19')).toBe('November 2026');
    expect(groupLabel('2026-10-31', '2026-11-19')).toBe('October 2026');
    const groups = groupByPeriod(
      [item({ date: '2026-11-18' }), item({ date: '2026-11-16' }), item({ date: '2026-11-12' }), item({ date: '2026-10-02' })],
      '2026-11-19',
    );
    expect(groups.map((g) => [g.label, g.items.length])).toEqual([
      ['This week', 2],
      ['Last week', 1],
      ['October 2026', 1],
    ]);
  });

  it('sorts with missing values last in both directions', () => {
    const rows = [item({ id: 'a', duration_s: 100 }), item({ id: 'b', duration_s: null }), item({ id: 'c', duration_s: 300 })];
    expect(sortActivities(rows, { key: 'duration', dir: 'desc' }).map((r) => r.id)).toEqual(['c', 'a', 'b']);
    expect(sortActivities(rows, { key: 'duration', dir: 'asc' }).map((r) => r.id)).toEqual(['a', 'c', 'b']);
  });
});

describe('series helpers', () => {
  it('spread skips gaps', () => {
    expect(spread([null, 100, 120, null, 140])).toEqual({ min: 100, max: 140, avg: 120 });
    expect(spread([null, null])).toBeNull();
  });
  it('pace leaves standing still as a gap', () => {
    const p = paceMinutes([3, 0.1, null]);
    expect(p[0]).toBeCloseTo(5.556, 2);
    expect(p[1]).toBeNull();
    expect(p[2]).toBeNull();
  });
  it('axes in minutes', () => {
    expect(minutesAxis([0, 30, 90])).toEqual([0, 0.5, 1.5]);
    const t = timedToMinutes([
      { time: '2026-11-18T17:00:00+00:00', value: 90 },
      { time: '2026-11-18T17:03:00+00:00', value: null },
    ]);
    expect(t).toEqual({ x: [0, 3], y: [90, null] });
    expect(timedToMinutes([])).toEqual({ x: [], y: [] });
  });
  it('zones: a zone the file lacks is unavailable, not 0', () => {
    const z = zoneEntries({ hr_zones: [{ zone: 2, duration_s: 540 }, { zone: 3, duration_s: null }] });
    expect(z.find((e) => e.zone === 2)?.minutes).toBe(9);
    expect(z.find((e) => e.zone === 2)?.text).toBe('9:00');
    expect(z.find((e) => e.zone === 3)?.minutes).toBeNull();
    expect(z.find((e) => e.zone === 5)?.minutes).toBeNull();
  });
});

describe('strength input parsing', () => {
  it('reps: empty clears, whole numbers only', () => {
    expect(parseReps('')).toEqual({ ok: true, value: null });
    expect(parseReps(' 12 ')).toEqual({ ok: true, value: 12 });
    expect(parseReps('1.5').ok).toBe(false);
    expect(parseReps('-1').ok).toBe(false);
    expect(parseReps('10001').ok).toBe(false);
    expect(parseReps('abc').ok).toBe(false);
  });
  it('weight: comma or dot, up to 2000', () => {
    expect(parseWeight('22,5')).toEqual({ ok: true, value: 22.5 });
    expect(parseWeight('')).toEqual({ ok: true, value: null });
    expect(parseWeight('2001').ok).toBe(false);
    expect(parseWeight('-3').ok).toBe(false);
    expect(parseWeight('1e3').ok).toBe(false);
  });
});

describe('strength edits', () => {
  const groups = [group([set(0), set(1, { edited: true, reps: 8 })])];

  it('validates reps, weight and the exercise name', () => {
    const drafts = draftsOf(groups);
    drafts[0] = { reps: 'x', weight: '2500', exercise: '   ' };
    const errors = validateAll(groups, drafts);
    expect(errors[0]?.reps).toBeTruthy();
    expect(errors[0]?.weight).toBeTruthy();
    expect(errors[0]?.exercise).toBeTruthy();
    expect(errors[1]).toBeUndefined();
  });

  it('an unchanged, unnamed exercise is not an error', () => {
    const g = [group([set(0, { exercise: null, original_exercise: null })], 'x')];
    expect(validateAll(g, draftsOf(g))).toEqual({});
  });

  it('sends only changed fields, null for cleared values', () => {
    const drafts = draftsOf(groups);
    drafts[0] = { reps: '12', weight: '', exercise: 'Bench press' };
    expect(buildPatch(groups, drafts)).toEqual([{ idx: 0, reps: 12, weight_kg: null }]);
  });

  it('sends the new exercise name trimmed', () => {
    const drafts = draftsOf(groups);
    drafts[0] = { reps: '10', weight: '22', exercise: '  Incline press ' };
    expect(buildPatch(groups, drafts)).toEqual([{ idx: 0, exercise: 'Incline press' }]);
  });

  it('sends nothing when nothing changed', () => {
    expect(buildPatch(groups, draftsOf(groups))).toEqual([]);
  });

  it('resetting an edited set to the watch values sends reset: true', () => {
    const drafts = draftsOf(groups);
    drafts[1] = { reps: '10', weight: '22', exercise: 'Bench press' };
    expect(buildPatch(groups, drafts)).toEqual([{ idx: 1, reset: true }]);
    expect(differsFromWatch(groups[0]!.sets[1]!, draftsOf(groups)[1]!)).toBe(true);
    expect(differsFromWatch(groups[0]!.sets[0]!, draftsOf(groups)[0]!)).toBe(false);
  });
});

describe('strength text and comparison', () => {
  it('describes reps and load', () => {
    expect(repsText([set(0), set(1), set(2)])).toBe('10, 10, 10 reps');
    expect(repsText([set(0, { reps: null, duration_s: 60 }), set(1, { reps: null, duration_s: 60 })])).toBe('60 s each');
    expect(repsText([set(0, { reps: null }), set(1, { reps: null })])).toBeNull();
    expect(loadText([set(0), set(1, { weight_kg: 24 })])).toBe('22 to 24 kg');
    expect(loadText([set(0, { weight_kg: null })])).toBeNull();
  });

  it('compares per exercise and lists the gap', () => {
    const now = [group([set(0), set(1)], 'Bench press'), group([set(2)], 'Row')];
    const before = [group([set(0, { weight_kg: 20 })], 'bench press'), group([set(1)], 'Squat')];
    const c = compareExercises(now, before);
    expect(c.rows.map((r) => [r.name, r.before?.sets ?? null])).toEqual([
      ['Bench press', 1],
      ['Row', null],
    ]);
    expect(c.rows[0]?.now.topWeight).toBe(22);
    expect(c.rows[0]?.before?.topWeight).toBe(20);
    expect(c.onlyBefore.map((e) => e.name)).toEqual(['Squat']);
  });

  it('delta text carries a sign and never hides a missing side', () => {
    expect(deltaText(5, 3)).toBe('+2');
    expect(deltaText(3, 5, ' kg')).toBe('-2 kg');
    expect(deltaText(3, 3)).toBe('same');
    expect(deltaText(null, 3)).toBeNull();
    expect(deltaText(3, null)).toBeNull();
  });
});

describe('timeTicks', () => {
  it('gives round steps from 0 with the unit on the last label', () => {
    expect(timeTicks(30).map((t) => t.label)).toEqual(['0', '10', '20', '30 min']);
    expect(timeTicks(30).map((t) => t.minutes)).toEqual([0, 10, 20, 30]);
    expect(timeTicks(45).map((t) => t.label)).toEqual(['0', '10', '20', '30', '40 min']);
    expect(timeTicks(2).map((t) => t.label)).toEqual(['0', '1', '2 min']);
  });
  it('switches to hours for long activities and never gives more than five ticks', () => {
    expect(timeTicks(190).map((t) => t.label)).toEqual(['0', '1', '2', '3 h']);
    for (const m of [3, 17, 59, 61, 95, 240, 600]) expect(timeTicks(m).length).toBeLessThanOrEqual(5);
  });
  it('has no ticks without a duration', () => {
    expect(timeTicks(0)).toEqual([]);
    expect(timeTicks(NaN)).toEqual([]);
  });
});

describe('timeLabels', () => {
  it('puts each time label at its minute on the chart x axis', () => {
    expect(timeLabels(30)).toEqual([
      { value: 0, text: '0' },
      { value: 10, text: '10' },
      { value: 20, text: '20' },
      { value: 30, text: '30 min' },
    ]);
  });
  it('has none without a duration', () => {
    expect(timeLabels(0)).toEqual([]);
  });
});

describe('listStats (the five figures of a desktop row)', () => {
  const labels = (a: ActivityListItem) => listStats(a).map((s) => s.label);
  const values = (a: ActivityListItem) => listStats(a).map((s) => s.value);

  it('a run: distance, time, pace, ascent, average heart rate', () => {
    expect(labels(item())).toEqual(['Distance', 'Time', 'Avg pace', 'Total ascent', 'Avg HR']);
    expect(values(item())).toEqual(['6.40 km', '38:00', '5:56 /km', '42 m', '151 bpm']);
  });
  it('a walk and a hike show the pace too', () => {
    expect(labels(item({ kind: 'walk' }))).toContain('Avg pace');
    expect(labels(item({ kind: 'hike' }))).toContain('Avg pace');
  });
  it('a ride shows the average speed worked out from distance and time', () => {
    const ride = item({ kind: 'cycle', distance_m: 10000, duration_s: 1800, avg_pace_s_per_km: null });
    expect(labels(ride)).toEqual(['Distance', 'Time', 'Avg speed', 'Total ascent', 'Avg HR']);
    expect(values(ride)[2]).toBe('20.0 km/h');
  });
  it('a swim and an unnamed sport lean on heart rate and energy', () => {
    expect(labels(item({ kind: 'swim', distance_m: 800 }))).toEqual(['Distance', 'Time', 'Avg HR', 'Max HR', 'Calories']);
    expect(values(item({ kind: 'swim', distance_m: 800 }))[0]).toBe('0.80 km');
    expect(labels(item({ kind: 'other', distance_m: null }))).toEqual(['Time', 'Distance', 'Avg HR', 'Max HR', 'Calories']);
  });
  it('strength: time, exercises, sets, volume, average heart rate', () => {
    const str = item({ kind: 'strength', distance_m: null, avg_pace_s_per_km: null, sets: 18, volume_kg: 3300.4, exercises: 6 });
    expect(labels(str)).toEqual(['Time', 'Exercises', 'Sets', 'Volume', 'Avg HR']);
    expect(values(str)).toEqual(['38:00', '6', '18', '3,300.4 kg', '151 bpm']);
  });
  it('a missing figure is null, never 0', () => {
    const empty = item({ duration_s: null, distance_m: null, avg_pace_s_per_km: null, avg_hr_bpm: null, ascent_m: null });
    expect(values(empty)).toEqual([null, null, null, null, null]);
    const str = item({ kind: 'strength', sets: null, volume_kg: null, exercises: null });
    expect(values(str).slice(1, 4)).toEqual([null, null, null]);
  });
});

describe('searchActivities', () => {
  const rows = [item({ id: '1' }), item({ id: '2', kind: 'strength', sport_name: 'training' }), item({ id: '3', kind: 'other', sport_name: 'stand_up_paddleboarding' })];
  it('keeps everything for an empty search', () => {
    expect(searchActivities(rows, '  ')).toHaveLength(3);
  });
  it('matches the type name, whatever the case', () => {
    expect(searchActivities(rows, 'RUN').map((r) => r.id)).toEqual(['1']);
    expect(searchActivities(rows, 'strength').map((r) => r.id)).toEqual(['2']);
  });
  it('matches the watch sport name of an unnamed type, underscores as spaces', () => {
    expect(searchActivities(rows, 'paddle').map((r) => r.id)).toEqual(['3']);
    expect(searchActivities(rows, 'up paddle').map((r) => r.id)).toEqual(['3']);
  });
  it('finds nothing for a word no row has', () => {
    expect(searchActivities(rows, 'swim')).toEqual([]);
  });
});

describe('desktop list sorting and dates', () => {
  it('every sort choice is found again from its Sort', () => {
    for (const c of SORT_CHOICES) expect(sortChoiceId(c.sort)).toBe(c.id);
  });
  it('writes the weekday and clock under a row name and a long date in the breadcrumb', () => {
    expect(weekdayClock({ date: '2026-11-23', start: '2026-11-23T18:14:00+00:00' })).toBe('Monday · 18:14');
    expect(longWhen({ date: '2026-11-08', start: '2026-11-08T17:22:00+00:00' })).toBe('November 8, 2026 @ 17:22');
    expect(longWhen({ date: '2026-11-08', start: '' })).toBe('November 8, 2026');
  });
});

describe('distance axis', () => {
  it('turns cumulative metres into kilometres, carrying a gap forward', () => {
    expect(distanceAxis([0, 500, null, 1500])).toEqual([0, 0.5, 0.5, 1.5]);
  });
  it('is not offered without a distance', () => {
    expect(distanceAxis([null, null])).toBeNull();
    expect(distanceAxis([0, 0])).toBeNull();
  });
  it('labels a few round kilometres, the unit on the last', () => {
    expect(distanceLabels(4.3).map((l) => l.text)).toEqual(['0', '1', '2', '3', '4 km']);
    expect(distanceLabels(0)).toEqual([]);
  });
});
