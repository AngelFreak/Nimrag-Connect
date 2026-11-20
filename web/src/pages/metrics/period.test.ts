import { describe, expect, it } from 'vitest';
import { METRICS, RANGE_DAYS, drillTarget, isMetricKey, metricHref, metricWindow, stepAnchor, weekStart } from './period';

const TODAY = '2026-11-19'; // a Thursday

describe('weekStart', () => {
  it('is the Monday on or before the day', () => {
    expect(weekStart('2026-11-16')).toBe('2026-11-16');
    expect(weekStart('2026-11-19')).toBe('2026-11-16');
    expect(weekStart('2026-11-22')).toBe('2026-11-16');
    expect(weekStart('2026-11-23')).toBe('2026-11-23');
  });
});

describe('metricWindow', () => {
  it('rolls back from the anchor: 7d, 4w, 12w, 6m and 1y are 7, 28, 84, 182 and 364 days', () => {
    expect(RANGE_DAYS).toEqual({ '1d': 1, '7d': 7, '4w': 28, '12w': 84, '6m': 182, '1y': 364 });
    expect(metricWindow('heart_rate', '1d', null, TODAY)).toMatchObject({ from: TODAY, to: TODAY, atToday: true });
    expect(metricWindow('heart_rate', '7d', null, TODAY)).toMatchObject({ from: '2026-11-13', to: TODAY });
    expect(metricWindow('heart_rate', '4w', null, TODAY)).toMatchObject({ from: '2026-10-23', to: TODAY });
    expect(metricWindow('vo2_max', '12w', null, TODAY)).toMatchObject({ from: '2026-08-28', to: TODAY });
    expect(metricWindow('vo2_max', '6m', null, TODAY)).toMatchObject({ from: '2026-05-22', to: TODAY });
    expect(metricWindow('steps', '1y', null, TODAY)).toMatchObject({ from: '2025-11-21', to: TODAY });
  });

  it('ends on `end` when it is an earlier day; today for a missing, bad or later one', () => {
    expect(metricWindow('steps', '7d', '2026-11-10', TODAY)).toEqual({ from: '2026-11-04', to: '2026-11-10', anchor: '2026-11-10', atToday: false });
    for (const end of [null, '', 'yesterday', '2026-13-40', '2026-11-19', '2026-12-31']) {
      expect(metricWindow('steps', '7d', end, TODAY)).toMatchObject({ to: TODAY, anchor: TODAY, atToday: true });
    }
  });

  it('is the calendar week for intensity minutes over 7d, whatever day of it is the anchor', () => {
    expect(metricWindow('intensity_minutes', '7d', null, TODAY)).toEqual({ from: '2026-11-16', to: '2026-11-22', anchor: TODAY, atToday: true });
    expect(metricWindow('intensity_minutes', '7d', '2026-11-12', TODAY)).toEqual({ from: '2026-11-09', to: '2026-11-15', anchor: '2026-11-12', atToday: false });
    // A Sunday that is today closes the week; there is no later one either.
    expect(metricWindow('intensity_minutes', '7d', null, '2026-11-22')).toMatchObject({ from: '2026-11-16', to: '2026-11-22', atToday: true });
    // Its other tabs roll like everyone else's.
    expect(metricWindow('intensity_minutes', '4w', null, TODAY)).toMatchObject({ from: '2026-10-23', to: TODAY });
  });
});

describe('stepAnchor', () => {
  it('moves by the length of the window and never past today', () => {
    expect(stepAnchor('7d', TODAY, -1, TODAY)).toBe('2026-11-12');
    expect(stepAnchor('4w', TODAY, -1, TODAY)).toBe('2026-10-22');
    expect(stepAnchor('1d', TODAY, -1, TODAY)).toBe('2026-11-18');
    expect(stepAnchor('7d', '2026-11-12', 1, TODAY)).toBe(TODAY);
    expect(stepAnchor('7d', '2026-11-05', 1, TODAY)).toBe('2026-11-12');
    expect(stepAnchor('1d', TODAY, 1, TODAY)).toBe(TODAY);
  });

  it('keeps an intensity week a week back: the day before the week is in the previous one', () => {
    const back = stepAnchor('7d', TODAY, -1, TODAY);
    expect(metricWindow('intensity_minutes', '7d', back, TODAY)).toMatchObject({ from: '2026-11-09', to: '2026-11-15' });
  });
});

describe('metricHref', () => {
  it('writes the tab and, for an earlier period, the day it ends on', () => {
    expect(metricHref('steps', '7d', null, TODAY)).toBe('/metrics/steps?range=7d');
    expect(metricHref('steps', '7d', TODAY, TODAY)).toBe('/metrics/steps?range=7d');
    expect(metricHref('heart_rate', '1d', '2026-11-12', TODAY)).toBe('/metrics/heart_rate?range=1d&end=2026-11-12');
  });
});

describe('drillTarget', () => {
  it('opens a day row on 1d for that day', () => {
    expect(drillTarget(METRICS.heart_rate, 'day', '2026-11-14', TODAY)).toEqual({ range: '1d', end: '2026-11-14' });
  });

  it('opens a week row on 7d, anchored on the last day of the week', () => {
    expect(drillTarget(METRICS.steps, 'week', '2026-09-07', TODAY)).toEqual({ range: '7d', end: '2026-09-13' });
  });

  it('keeps the current week from reaching past today', () => {
    expect(drillTarget(METRICS.steps, 'week', '2026-11-13', TODAY)).toEqual({ range: '7d', end: TODAY });
    expect(drillTarget(METRICS.intensity_minutes, 'week', '2026-11-16', TODAY)).toEqual({ range: '7d', end: TODAY });
  });

  it('leads nowhere on a page without that tab', () => {
    expect(drillTarget(METRICS.vo2_max, 'day', '2026-11-14', TODAY)).toBeNull();
    expect(drillTarget(METRICS.training_load, 'week', '2026-11-09', TODAY)).toBeNull();
  });

  it('is a tab of the page every time it leads somewhere', () => {
    for (const def of Object.values(METRICS)) {
      for (const bucket of ['day', 'week'] as const) {
        const t = drillTarget(def, bucket, '2026-11-02', TODAY);
        if (t) expect(def.ranges).toContain(t.range);
      }
    }
  });
});

describe('the pages', () => {
  it('are the nine metrics Nimrag has data for, each opening on a tab it has', () => {
    expect(Object.keys(METRICS)).toHaveLength(9);
    for (const def of Object.values(METRICS)) expect(def.ranges).toContain(def.defaultRange);
  });

  it('offer the tabs of the design: no 1y for energy and the performance pages, no current', () => {
    expect(METRICS.energy.ranges).toEqual(['1d', '7d', '4w']);
    expect(METRICS.respiration.ranges).toEqual(['1d', '7d', '4w']);
    expect(METRICS.spo2.ranges).toEqual(['1d', '7d', '4w']);
    expect(METRICS.vo2_max.ranges).toEqual(['4w', '12w', '6m']);
    expect(METRICS.heart_rate.ranges).toEqual(['1d', '7d', '4w', '1y']);
  });

  it('know which names are metrics', () => {
    expect(isMetricKey('spo2')).toBe(true);
    expect(isMetricKey('hrv')).toBe(false);
    expect(isMetricKey('toString')).toBe(false);
    expect(isMetricKey(undefined)).toBe(false);
  });
});
