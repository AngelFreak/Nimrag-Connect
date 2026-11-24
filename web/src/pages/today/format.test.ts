import { describe, expect, it } from 'vitest';
import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import { DEFAULT_HOME_CARDS, visibleCards } from './cards';
import { moveCard } from './EditHome';
import { activityDetail, fmtBytes, fmtDateTime, fmtPace, fmtSeconds, fmtTime, signed, syncLine, unit } from './format';

const activity = (over: Partial<ActivityListItem>): ActivityListItem => ({
  id: 'x',
  kind: 'run',
  sport_name: null,
  sub_sport_name: null,
  start: '2026-11-19T08:00:00+00:00',
  date: '2026-11-19',
  duration_s: null,
  distance_m: null,
  avg_pace_s_per_km: null,
  avg_hr_bpm: null,
  max_hr_bpm: null,
  calories_kcal: null,
  ascent_m: null,
  sets: null,
  volume_kg: null,
  exercises: null,
  ...over,
});

describe('formatting never turns a missing value into 0', () => {
  it('returns null for null and undefined', () => {
    expect(fmtSeconds(null)).toBeNull();
    expect(unit(undefined, 'bpm')).toBeNull();
    expect(signed(null)).toBeNull();
    expect(fmtPace(null)).toBeNull();
    expect(fmtBytes(null)).toBeNull();
    expect(fmtTime(null, 'Etc/UTC')).toBeNull();
    expect(fmtDateTime('not a date', 'Etc/UTC')).toBeNull();
  });

  it('keeps a real zero', () => {
    expect(unit(0, 'min')).toBe('0 min');
    expect(signed(0)).toBe('0');
  });

  it('formats values the way the design shows them', () => {
    expect(fmtSeconds(25920)).toBe('7h 12m');
    expect(unit(62, 'ms')).toBe('62 ms');
    expect(unit(77.6, 'kg', 1)).toBe('77.6 kg');
    expect(signed(54)).toBe('+54');
    expect(signed(-31)).toBe('−31');
    expect(fmtPace(356)).toBe('5:56 /km');
    expect(fmtBytes(222298112)).toBe('212 MB');
    expect(fmtBytes(0)).toBe('0 B');
  });

  it('shows times in the owner’s zone, not the browser’s', () => {
    expect(fmtTime('2026-11-19T06:42:00+00:00', 'Europe/Copenhagen')).toBe('07:42');
    expect(fmtDateTime('2026-11-19T06:42:00+00:00', 'Europe/Copenhagen')).toBe('19 Nov, 07:42');
  });

  it('syncLine falls back from last sync to latest data to nothing', () => {
    expect(syncLine('2026-11-19T07:42:00+00:00', null, 'Etc/UTC')).toBe('Watch synced 07:42');
    expect(syncLine(null, '2026-11-19T22:15:00+00:00', 'Etc/UTC')).toBe('Latest watch data 22:15');
    expect(syncLine(null, null, 'Etc/UTC')).toBe('No watch data yet');
  });
});

describe('activity detail', () => {
  it('strength gives sets and volume, a run gives distance and pace, nothing gives null', () => {
    expect(activityDetail(activity({ kind: 'strength', sets: 18, volume_kg: 3300 }))).toBe('18 sets · 3,300 kg');
    expect(activityDetail(activity({ distance_m: 6400, avg_pace_s_per_km: 356 }))).toBe('6.40 km · 5:56 /km');
    expect(activityDetail(activity({}))).toBeNull();
  });
});

describe('home card layout helpers', () => {
  it('default layout holds each of the 16 cards once, with the three optional ones hidden', () => {
    expect(new Set(DEFAULT_HOME_CARDS.order).size).toBe(16);
    expect(DEFAULT_HOME_CARDS.order).toHaveLength(16);
    expect(DEFAULT_HOME_CARDS.order.slice(0, 3)).toEqual(['energy', 'training_readiness', 'sleep']);
    expect(visibleCards(DEFAULT_HOME_CARDS)).toHaveLength(13);
  });

  it('visibleCards keeps the order and drops hidden ids', () => {
    expect(visibleCards({ order: ['steps', 'sleep', 'energy'], hidden: ['sleep'] })).toEqual(['steps', 'energy']);
  });

  it('moveCard moves one id and ignores moves off either end', () => {
    expect(moveCard(['energy', 'sleep', 'steps'], 'steps', 0)).toEqual(['steps', 'energy', 'sleep']);
    const same = ['energy', 'sleep'] as const;
    expect(moveCard([...same], 'energy', -1)).toEqual(same);
    expect(moveCard([...same], 'sleep', 2)).toEqual(same);
  });
});
