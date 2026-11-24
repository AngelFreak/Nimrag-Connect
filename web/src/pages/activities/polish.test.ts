import { describe, expect, it } from 'vitest';
import type { ActivityDetail } from '../../api/generated';
import { colorRuns } from '../../lib/colorRuns';
import { zoneLimits, zoneOf } from './lapZones';
import { defaultRouteChoice, metricValues, quantile, rampColor, routeChoices, routeColors, routeLegend, routeScale } from './routeColor';
import { sampleAt } from './series';
import { headlineFor, statGroupsFor } from './summary';
import { teFill, teRows, teWord } from './trainingEffect';

describe('route colours', () => {
  it('clips the scale to the 5th to 95th percentile so one spike does not flatten it', () => {
    const values = [...Array.from({ length: 98 }, (_, i) => 2 + i * 0.01), 2.5, 40];
    const scale = routeScale(values)!;
    expect(scale.hi).toBeLessThan(3.1);
    expect(scale.lo).toBeGreaterThan(2);
    expect(quantile([1, 2, 3, 4, 5], 0.5)).toBe(3);
  });
  it('has no scale for a flat or too short series', () => {
    expect(routeScale([3, 3, 3])).toBeNull();
    expect(routeScale([3, null])).toBeNull();
  });
  it('colours fast warm and slow cool; values beyond the scale take the end colours', () => {
    const scale = { lo: 2, hi: 4 };
    const [slow, mid, fast, beyond] = routeColors([2, 3, 4, 9], scale);
    expect(slow).toBe(rampColor(0));
    expect(fast).toBe(rampColor(1));
    expect(mid).not.toBe(slow);
    expect(beyond).toBe(fast);
    expect(rampColor(0)).toBe('#3a6fe8');
    expect(rampColor(1)).toBe('#e5484d');
  });
  it('a point without a value keeps the colour before it', () => {
    const c = routeColors([null, 2, null, 4], { lo: 2, hi: 4 });
    expect(c[0]).toBe(c[1]);
    expect(c[2]).toBe(c[1]);
    expect(c[3]).toBe(rampColor(1));
  });
  it('merges equal colours into runs that share their joining point', () => {
    expect(colorRuns(['a', 'a', 'b', 'b', 'c'])).toEqual([
      { color: 'a', from: 0, to: 2 },
      { color: 'b', from: 2, to: 4 },
    ]);
  });
  it('writes the legend ends from slow to fast, so the cool end is the slow one', () => {
    const legend = routeLegend('pace', { lo: 1000 / 390, hi: 1000 / 290 });
    expect(legend).toMatchObject({ low: '6:30 /km', high: '4:50 /km' });
    expect(routeLegend('hr', { lo: 111.6, hi: 171 })).toMatchObject({ low: '112 bpm', high: '171 bpm' });
    expect(routeLegend('speed', { lo: 5, hi: 9 }).low).toBe('18.0 km/h');
  });
  it('treats standing still as a gap in pace', () => {
    expect(metricValues('pace', { speed_mps: [0.2, 0.2, 0.2] })).toEqual([null, null, null]);
  });
  it('picks the choices and the default by sport', () => {
    const series = { speed_mps: [2, 3, 2.5], heart_rate_bpm: [100, 150, 140], altitude_m: [null, null, null] };
    expect(routeChoices('run', series)).toEqual(['pace', 'hr', 'plain']);
    expect(routeChoices('cycle', series)).toEqual(['speed', 'hr', 'plain']);
    expect(routeChoices('run', null)).toEqual([]);
    expect(defaultRouteChoice('run', ['pace', 'plain'])).toBe('pace');
    expect(defaultRouteChoice('cycle', ['speed', 'plain'])).toBe('speed');
    expect(defaultRouteChoice('hike', ['hr', 'plain'])).toBe('plain');
    expect(defaultRouteChoice('training', ['hr', 'plain'])).toBe('plain');
  });
});

describe('training effect words', () => {
  it('changes word at each boundary', () => {
    const words: [number, string][] = [
      [0, 'No effect'],
      [0.9, 'No effect'],
      [1.0, 'Minor'],
      [1.9, 'Minor'],
      [2.0, 'Maintaining'],
      [2.9, 'Maintaining'],
      [3.0, 'Improving'],
      [3.9, 'Improving'],
      [4.0, 'Highly improving'],
      [4.9, 'Highly improving'],
      [5.0, 'Overreaching'],
    ];
    for (const [v, w] of words) expect(teWord(v), String(v)).toBe(w);
  });
  it('reads the value as shown: 0.95 is 1.0', () => {
    expect(teWord(0.95)).toBe('Minor');
  });
  it('fills five steps in proportion', () => {
    expect(teFill(3.4)).toEqual([1, 1, 1, 0.4, 0]);
    expect(teFill(5)).toEqual([1, 1, 1, 1, 1]);
    expect(teFill(0)).toEqual([0, 0, 0, 0, 0]);
  });
  it('lists only the effects that have a value', () => {
    expect(teRows({ aerobic_te: null, anaerobic_te: 0.2 }).map((r) => r.key)).toEqual(['anaerobic']);
  });
});

function act(over: Partial<ActivityDetail>): ActivityDetail {
  return {
    id: 'x',
    kind: 'run',
    sport_name: null,
    sub_sport_name: null,
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
    sample: false,
    end: null,
    elapsed_s: 2280,
    avg_speed_mps: 2.8,
    max_speed_mps: 4,
    descent_m: 40,
    avg_cadence: 86,
    aerobic_te: null,
    anaerobic_te: null,
    training_load: null,
    vo2max_derived: null,
    has_track: false,
    has_strength: false,
    hr_below_zone1_s: null,
    hr_zones: [],
    laps: [],
    series: null,
    reps: null,
    ...over,
  };
}

describe('summary by sport', () => {
  const labels = (a: ActivityDetail) => headlineFor(a).map((h) => h.label);
  it('run and ride: distance, time, pace or speed, heart rate, ascent', () => {
    expect(labels(act({}))).toEqual(['Distance', 'Time', 'Avg pace', 'Avg HR', 'Ascent']);
    expect(labels(act({ kind: 'cycle' }))).toEqual(['Distance', 'Time', 'Avg speed', 'Avg HR', 'Ascent']);
  });
  it('strength: time, sets, reps, heart rate', () => {
    const a = act({ kind: 'strength', sets: 18, reps: 160 });
    expect(labels(a)).toEqual(['Time', 'Sets', 'Reps', 'Avg HR']);
    expect(headlineFor(a)[2]?.value).toBe('160');
  });
  it('other: time, heart rate, energy', () => {
    expect(labels(act({ kind: 'training' }))).toEqual(['Time', 'Avg HR', 'Energy']);
  });
  it('leaves empty values out of the stats and does not repeat the headline', () => {
    const none = statGroupsFor(act({ descent_m: null, avg_cadence: null, max_speed_mps: null, avg_speed_mps: null, max_hr_bpm: null, calories_kcal: null }));
    expect(none).toEqual([]);
    const some = statGroupsFor(act({}));
    const all = some.flatMap((g) => g.items.map((i) => i.label));
    expect(all).not.toContain('Avg HR');
    expect(all).not.toContain('Ascent');
    expect(all).toContain('Descent');
    expect(some.every((g) => g.items.every((i) => i.value))).toBe(true);
  });
});

describe('heart rate zones of laps', () => {
  const hr = Array.from({ length: 100 }, (_, i) => 100 + i);
  const zones = [1, 2, 3, 4, 5].map((zone) => ({ zone, duration_s: 20 }));
  it('reads the zone limits from the share of time in each zone', () => {
    const limits = zoneLimits(hr, zones, null)!;
    expect(limits.map(Math.round)).toEqual([100, 120, 140, 159, 179]);
    expect(zoneOf(150, limits)).toBe(3);
    expect(zoneOf(99, limits)).toBe(0);
  });
  it('gives none without time in zones or samples', () => {
    expect(zoneLimits(hr, [], null)).toBeNull();
    expect(zoneLimits([100, 120], zones, null)).toBeNull();
  });
});

describe('the value under the pointer', () => {
  it('writes every value as shown, a gap as null', () => {
    const series = { elapsed_s: [0, 750], heart_rate_bpm: [100, 148], speed_mps: [0.1, 3.2], altitude_m: [null, 42], distance_m: [0, 2410], cadence: [null, 85] };
    expect(sampleAt(series, 1)).toMatchObject({ time: '12:30', distance: '2.41 km', pace: '5:13 /km', hr: '148 bpm', elevation: '42 m', cadence: '85 /min' });
    expect(sampleAt(series, 0)).toMatchObject({ pace: null, elevation: null });
    expect(sampleAt(series, null)).toBeNull();
    expect(sampleAt(series, 5)).toBeNull();
  });
});
