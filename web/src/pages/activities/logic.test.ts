import { describe, expect, it } from 'vitest';
import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import type { ExerciseGroup } from '../../api/generated/ExerciseGroup';
import type { StrengthSet } from '../../api/generated/StrengthSet';
import { activityTitle, groupByPeriod, groupLabel, parseType, rowText, sortActivities, weekStart, whenLabel } from './format';
import { minutesAxis, paceMinutes, spread, timedToMinutes } from './series';
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
    expect(rowText(item())).toEqual({ value: '6.4 km', sub: '38:00 · 5:56 /km', detail: '6.4 km · 5:56 /km' });
    expect(rowText(item({ kind: 'strength', distance_m: null, avg_pace_s_per_km: null, duration_s: 2520, sets: 18, volume_kg: 3300 }))).toEqual({
      value: '42 min',
      sub: '18 sets · 3,300 kg',
      detail: '18 sets · 3,300 kg',
    });
    expect(rowText(item({ kind: 'walk', duration_s: 3120, distance_m: 4800, avg_pace_s_per_km: 650 })).sub).toBe('52 min');
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
