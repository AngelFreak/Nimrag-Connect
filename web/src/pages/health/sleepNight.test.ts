import { describe, expect, it } from 'vitest';
import type { SleepNight } from '../../api/generated/SleepNight';
import { fmtHours, headlineFor, longAwakenings, rateAwake, rateDuration, rateShare, rateStress, ratingOfScore, sleepFactors, weakestFactor, type Factor } from './sleepNight';

const NONE = { overall: null, quality: null, duration: null, recovery: null, deep: null, light: null, rem: null };

/** A night with 8 h asleep, healthy stages (deep 20 %, light 50 %, REM 30 %... see below), calm and unbroken. */
function night(over: Partial<SleepNight> = {}): SleepNight {
  const asleep = 8 * 3600;
  return {
    date: '2026-11-19',
    start: '2026-11-18T23:00:00+01:00',
    end: '2026-11-19T07:30:00+01:00',
    duration_s: asleep,
    time_in_bed_s: asleep + 1800,
    score: 85,
    score_label: 'good',
    scores: NONE,
    awakenings: 1,
    avg_stress: 14,
    stages_available: true,
    deep_s: asleep * 0.2,
    light_s: asleep * 0.5,
    rem_s: asleep * 0.3,
    awake_s: 15 * 60,
    stages: [],
    hrv_ms: null,
    lowest_hr_bpm: null,
    breathing_brpm: null,
    spo2_avg_percent: null,
    spo2_low_percent: null,
    sleep_need_min: 480,
    restless_moments: null,
    avg_hr_bpm: null,
    energy_change: null,
    breathing_low_brpm: null,
    hrv_status: null,
    ...over,
  };
}

const factor = (n: SleepNight, key: string) => sleepFactors(n).find((f) => f.key === key);

describe('score bands', () => {
  it('rates 90, 80 and 60 as the lowest of Excellent, Good and Fair', () => {
    expect([100, 90, 89.9, 80, 79.9, 60, 59.9, 0].map(ratingOfScore)).toEqual(['excellent', 'excellent', 'good', 'good', 'fair', 'fair', 'poor', 'poor']);
    expect(ratingOfScore(null)).toBeNull();
    expect(ratingOfScore(Number.NaN)).toBeNull();
  });
});

describe('duration against the need', () => {
  it('has its boundaries at the need, 30 min under, 90 min under and 2 h over', () => {
    expect(rateDuration(480, 480)).toBe('excellent');
    expect(rateDuration(600, 480)).toBe('excellent'); // exactly 2 h over still counts
    expect(rateDuration(601, 480)).toBe('fair');
    expect(rateDuration(479, 480)).toBe('good');
    expect(rateDuration(450, 480)).toBe('good');
    expect(rateDuration(449, 480)).toBe('fair');
    expect(rateDuration(390, 480)).toBe('fair');
    expect(rateDuration(389, 480)).toBe('poor');
  });
  it('follows a different need', () => {
    expect(rateDuration(420, 420)).toBe('excellent');
    expect(rateDuration(480, 360)).toBe('excellent');
    expect(rateDuration(480, 360)).toBe('excellent'); // exactly 2 h over
    expect(rateDuration(481, 360)).toBe('fair');
  });
});

describe('stress while asleep', () => {
  it('has its boundaries at 15, 20 and 30', () => {
    expect([0, 15, 15.1, 20, 20.1, 30, 30.1].map(rateStress)).toEqual(['excellent', 'excellent', 'good', 'good', 'fair', 'fair', 'poor']);
  });
});

describe('stage shares', () => {
  it('is Excellent inside the band and edges included', () => {
    expect(rateShare(16, 16, 33)).toBe('excellent');
    expect(rateShare(33, 16, 33)).toBe('excellent');
    expect(rateShare(25, 16, 33)).toBe('excellent');
  });
  it('steps down at 4 and 8 points outside, on both sides', () => {
    expect(rateShare(12, 16, 33)).toBe('good'); // 4 under
    expect(rateShare(11.9, 16, 33)).toBe('fair');
    expect(rateShare(8, 16, 33)).toBe('fair'); // 8 under
    expect(rateShare(7.9, 16, 33)).toBe('poor');
    expect(rateShare(37, 16, 33)).toBe('good'); // 4 over
    expect(rateShare(37.1, 16, 33)).toBe('fair');
    expect(rateShare(41, 16, 33)).toBe('fair');
    expect(rateShare(41.1, 16, 33)).toBe('poor');
  });
  it('uses the light and REM bands', () => {
    expect(rateShare(30, 30, 64)).toBe('excellent');
    expect(rateShare(64, 30, 64)).toBe('excellent');
    expect(rateShare(68, 30, 64)).toBe('good');
    expect(rateShare(21, 21, 31)).toBe('excellent');
    expect(rateShare(31.5, 21, 31)).toBe('good');
  });
});

describe('awake and restless', () => {
  it('is Excellent under 20 min with at most one long awakening', () => {
    expect(rateAwake(19, 0)).toBe('excellent');
    expect(rateAwake(19.9, 1)).toBe('excellent');
    expect(rateAwake(19, 2)).toBe('good');
    expect(rateAwake(20, 0)).toBe('good');
  });
  it('steps down at 30 and 45 min', () => {
    expect(rateAwake(29.9, 5)).toBe('good');
    expect(rateAwake(30, 0)).toBe('fair');
    expect(rateAwake(44.9, 0)).toBe('fair');
    expect(rateAwake(45, 0)).toBe('poor');
  });
  it('counts awake stretches over 5 minutes from the stages, else the watch count', () => {
    const awake = (a: string, b: string) => ({ start: `2026-11-19T${a}:00+01:00`, end: `2026-11-19T${b}:00+01:00`, stage: 'awake' as const });
    const n = night({ stages: [awake('01:00', '01:05'), awake('02:00', '02:06'), awake('03:00', '03:20')], awakenings: 9 });
    expect(longAwakenings(n)).toBe(2);
    expect(longAwakenings(night({ stages: [], stages_available: false, awakenings: 3 }))).toBe(3);
    expect(longAwakenings(night({ stages: [], stages_available: false, awakenings: null }))).toBe(0);
  });
});

describe('the factor list', () => {
  it('lists six factors for a full night, in a fixed order', () => {
    expect(sleepFactors(night()).map((f) => f.key)).toEqual(['duration', 'stress', 'deep', 'light', 'rem', 'awake']);
  });

  it('rates by our model with exact values', () => {
    const n = night({ duration_s: 7 * 3600 + 20 * 60, deep_s: 3600 * 1.2, light_s: 3600 * 4, rem_s: 3600 * 2 + 0, awake_s: 25 * 60, avg_stress: 22, awakenings: 3, restless_moments: 14 });
    const asleep = n.deep_s! + n.light_s! + n.rem_s!;
    expect(asleep).toBe(7.2 * 3600);
    const f = (k: string) => factor(n, k)!;
    expect(f('duration')).toMatchObject({ rating: 'fair', value: '7h 20m of 8 h', fromWatch: false, side: 'low' });
    expect(f('stress')).toMatchObject({ rating: 'fair', value: '22 average' });
    // deep 1h12m of 7.2h is 16.7 %: inside 16 to 33
    expect(f('deep')).toMatchObject({ rating: 'excellent', value: '1h 12m · 16.7 %' });
    // light 4h of 7.2h is 55.6 %
    expect(f('light').rating).toBe('excellent');
    // REM 2h of 7.2h is 27.8 %
    expect(f('rem').rating).toBe('excellent');
    expect(f('awake')).toMatchObject({ rating: 'good', value: '25m awake · 3 awakenings · 14 restless' });
  });

  it('hides stress without a value and any stage row without data', () => {
    const n = night({ avg_stress: null, deep_s: null, light_s: null, rem_s: null, awake_s: null, awakenings: null });
    expect(sleepFactors(n).map((f) => f.key)).toEqual(['duration']);
  });

  it('uses the watch sub-score instead of the model, and says so', () => {
    const n = night({ scores: { ...NONE, duration: 95, deep: 79, light: 60, rem: 59, recovery: 85, quality: 40 } });
    const rated = Object.fromEntries(sleepFactors(n).map((f) => [f.key, f.rating]));
    expect(rated).toEqual({ duration: 'excellent', stress: 'good', deep: 'fair', light: 'fair', rem: 'poor', awake: 'poor' });
    expect(sleepFactors(n).every((f) => f.fromWatch && f.target.startsWith('Your watch scored this'))).toBe(true);
  });

  it('explains a watch rating with its score and the next level, then the target band', () => {
    const n = night({ scores: { ...NONE, duration: 72, deep: 95 } });
    expect(factor(n, 'duration')!.target).toBe('Your watch scored this 72 of 100; Good is 80 or more. Your need is 8 h.');
    expect(factor(n, 'deep')!.target).toBe('Your watch scored this 95 of 100; Excellent is 90 or more. Target 16 to 33 % of your time asleep.');
    expect(factor(n, 'duration')!.watchScore).toBe(72);
  });

  it('does not show our own rule text on a watch-rated row', () => {
    const n = night({ scores: { ...NONE, duration: 72, recovery: 70, deep: 70, light: 70, rem: 70, quality: 70 } });
    for (const f of sleepFactors(n)) expect(f.target).not.toMatch(/Within \d+ (points|min)|Rated from/);
  });

  it('keeps a watch sub-score row even without stage times', () => {
    const n = night({ deep_s: null, light_s: null, rem_s: null, scores: { ...NONE, deep: 91 } });
    expect(factor(n, 'deep')).toMatchObject({ rating: 'excellent', value: 'No stage data', fromWatch: true });
    expect(factor(n, 'light')).toBeUndefined();
  });

  it('puts the target band in one line', () => {
    expect(factor(night(), 'deep')!.target).toContain('Target 16 to 33 %');
    expect(factor(night(), 'light')!.target).toContain('Target 30 to 64 %');
    expect(factor(night(), 'rem')!.target).toContain('Target 21 to 31 %');
    expect(factor(night({ sleep_need_min: 450 }), 'duration')!.target).toContain('7 h 30 min');
  });
});

describe('the weakest factor', () => {
  const f = (key: Factor['key'], rating: Factor['rating']): Factor => ({ key, name: key, value: '', rating, fromWatch: false, watchScore: null, target: '', side: 'in' });
  it('is the lowest rating', () => {
    expect(weakestFactor([f('duration', 'good'), f('deep', 'fair'), f('rem', 'poor')])?.key).toBe('rem');
  });
  it('breaks ties awake, deep, REM, stress, light, then duration', () => {
    const all = (r: Factor['rating']) => (['duration', 'stress', 'deep', 'light', 'rem', 'awake'] as const).map((k) => f(k, r));
    expect(weakestFactor(all('fair'))?.key).toBe('awake');
    expect(weakestFactor(all('fair').filter((x) => x.key !== 'awake'))?.key).toBe('deep');
    expect(weakestFactor(all('fair').filter((x) => !['awake', 'deep'].includes(x.key)))?.key).toBe('rem');
    expect(weakestFactor(all('fair').filter((x) => !['awake', 'deep', 'rem'].includes(x.key)))?.key).toBe('stress');
    expect(weakestFactor(all('fair').filter((x) => ['light', 'duration'].includes(x.key)))?.key).toBe('light');
    expect(weakestFactor([])).toBeNull();
  });
});

describe('the headline, by rule', () => {
  const h = (n: SleepNight) => headlineFor(n);

  it('1: more than 1 h short is a short night, with the exact gap', () => {
    const r = h(night({ duration_s: 6 * 3600 + 59 * 60 }));
    expect(r.rule).toBe('short_night');
    expect(r.title).toBe('Short night');
    expect(r.sentence).toContain('1 h 1 min less than your 8 h need');
  });
  it('1: a watch-rated Good duration never opens with "Short night"', () => {
    const r = h(night({ duration_s: 6 * 3600, scores: { ...NONE, duration: 82 } }));
    expect(r.rule).not.toBe('short_night');
    expect(r.sentence).toContain('a bit shorter than your 8 h need');
    // a watch-rated Fair duration still does
    expect(h(night({ duration_s: 6 * 3600, scores: { ...NONE, duration: 70 } })).rule).toBe('short_night');
  });
  it('1: exactly 1 h short is not yet a short night', () => {
    expect(h(night({ duration_s: 7 * 3600 })).rule).not.toBe('short_night');
    expect(h(night({ duration_s: 7 * 3600 - 60 })).rule).toBe('short_night');
  });
  it('1: more than 3 h short is a very short night', () => {
    expect(h(night({ duration_s: 4 * 3600 })).title).toBe('Very short night');
  });
  it('1: more than 1 h over is a long night; more than 3 h over a very long one', () => {
    expect(h(night({ duration_s: 9 * 3600 + 60 })).rule).toBe('long_night');
    expect(h(night({ duration_s: 9 * 3600 })).rule).not.toBe('long_night');
    expect(h(night({ duration_s: 10 * 3600 })).title).toBe('Long night');
    expect(h(night({ duration_s: 11 * 3600 + 1 })).title).toBe('Very long night');
  });
  it('1: a different need moves the line', () => {
    expect(h(night({ sleep_need_min: 420, duration_s: 6 * 3600 - 60 })).rule).toBe('short_night');
    expect(h(night({ sleep_need_min: 360 })).rule).toBe('long_night');
  });

  it('2: restless names the night when awake time is the weakest', () => {
    expect(h(night({ awake_s: 50 * 60 })).title).toBe('Restless night');
    expect(h(night({ awake_s: 35 * 60 })).title).toBe('A few wake-ups');
  });
  it('2: little deep sleep, and a lot of it', () => {
    const asleep = 8 * 3600;
    expect(h(night({ deep_s: asleep * 0.03, light_s: asleep * 0.67, rem_s: asleep * 0.3 })).title).toBe('Little deep sleep');
    expect(h(night({ deep_s: asleep * 0.11, light_s: asleep * 0.59, rem_s: asleep * 0.3 })).title).toBe('Less deep sleep');
    expect(h(night({ deep_s: asleep * 0.5, light_s: asleep * 0.2, rem_s: asleep * 0.3 })).rule).toBe('lots_deep');
  });
  it('2: little REM sleep, and a lot of it', () => {
    const asleep = 8 * 3600;
    expect(h(night({ deep_s: asleep * 0.22, light_s: asleep * 0.74, rem_s: asleep * 0.04 })).title).toBe('Little REM sleep');
    expect(h(night({ deep_s: asleep * 0.2, light_s: asleep * 0.65, rem_s: asleep * 0.15 })).title).toBe('Slightly low REM');
    expect(h(night({ deep_s: asleep * 0.2, light_s: asleep * 0.25, rem_s: asleep * 0.55 })).rule).toBe('lots_rem');
  });
  it('2: tense when stress is the weakest', () => {
    expect(h(night({ avg_stress: 45 })).title).toBe('Tense night');
    expect(h(night({ avg_stress: 25 })).title).toBe('A little tense');
  });
  it('2: light and shallow when light sleep is the weakest and too large a share', () => {
    const asleep = 8 * 3600;
    // light 70 %: 6 points over (Fair); deep 13 % and REM 17 % are within 4 points
    const r = h(night({ deep_s: asleep * 0.13, light_s: asleep * 0.7, rem_s: asleep * 0.17 }));
    expect(r.rule).toBe('light_shallow');
    expect(r.title).toBe('On the light side');
    const poor = h(night({ deep_s: asleep * 0.13, light_s: asleep * 0.7, rem_s: asleep * 0.17, scores: { ...NONE, light: 40 } }));
    expect(poor.title).toBe('Light, shallow sleep');
  });
  it('2: uneven stages when the watch rates light sleep low and its share is small', () => {
    const asleep = 8 * 3600;
    const r = h(night({ deep_s: asleep * 0.35, light_s: asleep * 0.25, rem_s: asleep * 0.4, scores: { ...NONE, deep: 95, rem: 95, light: 50, duration: 95, quality: 95, recovery: 95 } }));
    expect(r.rule).toBe('uneven_light');
  });
  it('2: a slightly short night with everything else good is "a little short"', () => {
    const r = h(night({ duration_s: 7 * 3600 + 15 * 60, deep_s: 7.25 * 3600 * 0.2, light_s: 7.25 * 3600 * 0.5, rem_s: 7.25 * 3600 * 0.3 }));
    expect(r.rule).toBe('a_bit_short');
    expect(r.sentence).toContain('8 h');
  });
  it('2: ties go to awake before deep', () => {
    const asleep = 8 * 3600;
    const r = h(night({ awake_s: 50 * 60, deep_s: asleep * 0.03, light_s: asleep * 0.67, rem_s: asleep * 0.3 }));
    expect(r.rule).toBe('restless');
  });

  it('3: all factors Good or better is a positive headline', () => {
    expect(h(night()).rule).toBe('solid');
    expect(h(night()).title).toBe('Solid, restful night');
    expect(h(night({ score: 93 })).title).toBe('Excellent, restful night');
  });

  it('every sentence is plain, short and never medical', () => {
    const asleep = 8 * 3600;
    const cases: SleepNight[] = [
      night({ duration_s: 5 * 3600 }),
      night({ duration_s: 6 * 3600 + 30 * 60 }),
      night({ duration_s: 9 * 3600 + 30 * 60 }),
      night({ duration_s: 11.5 * 3600 }),
      night({ awake_s: 50 * 60 }),
      night({ awake_s: 35 * 60 }),
      night({ deep_s: asleep * 0.03, light_s: asleep * 0.67, rem_s: asleep * 0.3 }),
      night({ deep_s: asleep * 0.11, light_s: asleep * 0.59, rem_s: asleep * 0.3 }),
      night({ deep_s: asleep * 0.5, light_s: asleep * 0.2, rem_s: asleep * 0.3 }),
      night({ deep_s: asleep * 0.22, light_s: asleep * 0.74, rem_s: asleep * 0.04 }),
      night({ deep_s: asleep * 0.2, light_s: asleep * 0.65, rem_s: asleep * 0.15 }),
      night({ deep_s: asleep * 0.2, light_s: asleep * 0.25, rem_s: asleep * 0.55 }),
      night({ deep_s: asleep * 0.13, light_s: asleep * 0.7, rem_s: asleep * 0.17 }),
      night({ deep_s: asleep * 0.13, light_s: asleep * 0.7, rem_s: asleep * 0.17, scores: { ...NONE, light: 40 } }),
      night({ deep_s: asleep * 0.35, light_s: asleep * 0.25, rem_s: asleep * 0.4, scores: { ...NONE, deep: 95, rem: 95, light: 50, duration: 95, quality: 95, recovery: 95 } }),
      night({ avg_stress: 45 }),
      night({ avg_stress: 25 }),
      night({ deep_s: asleep * 0.3, light_s: asleep * 0.15, rem_s: asleep * 0.55 }),
      night({ deep_s: asleep * 0.14, light_s: asleep * 0.86, rem_s: asleep * 0.0001 }),
      night({ duration_s: 7 * 3600 + 15 * 60, deep_s: 7.25 * 3600 * 0.2, light_s: 7.25 * 3600 * 0.5, rem_s: 7.25 * 3600 * 0.3 }),
      night(),
      night({ score: 95 }),
    ];
    const rules = new Set<string>();
    for (const c of cases) {
      const r = h(c);
      rules.add(`${r.rule}:${r.title}`);
      expect(r.title.split(' ').length).toBeGreaterThanOrEqual(2);
      expect(r.title.split(' ').length).toBeLessThanOrEqual(6);
      expect(r.sentence.endsWith('.')).toBe(true);
      expect(r.sentence).not.toMatch(/disease|diagnos|treat|cure|doctor|disorder|insomnia|apnea|medic/i);
    }
    expect(rules.size).toBeGreaterThanOrEqual(15);
  });
});

describe('fmtHours', () => {
  it('writes whole hours and minutes', () => {
    expect([480, 465, 45, 60, 0].map(fmtHours)).toEqual(['8 h', '7 h 45 min', '45 min', '1 h', '0 min']);
  });
});
