import { afterEach, describe, expect, it } from 'vitest';
import { addDays, addMonths, fmtShortDay, isIsoDay, isIsoMonth } from './dates';
import { fmtClock, fmtDurationMin, fmtInt, isMissing, withUnit } from './format';
import { readNativeStatus } from '../native';
import { safeNext } from '../pages/LoginPage';

describe('format: missing is null, never 0', () => {
  it('returns null for null, undefined and NaN', () => {
    expect(fmtInt(null)).toBeNull();
    expect(fmtInt(undefined)).toBeNull();
    expect(fmtInt(Number.NaN)).toBeNull();
    expect(fmtDurationMin(null)).toBeNull();
    expect(fmtClock(undefined)).toBeNull();
    expect(withUnit(null, 'ms')).toBeNull();
  });
  it('keeps real zeros', () => {
    expect(fmtInt(0)).toBe('0');
    expect(isMissing(0)).toBe(false);
  });
  it('formats durations and numbers', () => {
    expect(fmtDurationMin(432)).toBe('7h 12m');
    expect(fmtDurationMin(12)).toBe('12m');
    expect(fmtClock(2280)).toBe('38:00');
    expect(fmtClock(3723)).toBe('1:02:03');
    expect(fmtInt(6240)).toBe('6,240');
    expect(withUnit('62', 'ms')).toBe('62 ms');
  });
});

describe('dates', () => {
  it('validates days and months', () => {
    expect(isIsoDay('2026-11-19')).toBe(true);
    expect(isIsoDay('2026-02-30')).toBe(false);
    expect(isIsoDay(undefined)).toBe(false);
    expect(isIsoMonth('2026-11')).toBe(true);
    expect(isIsoMonth('2026-13')).toBe(false);
  });
  it('adds days and months across boundaries', () => {
    expect(addDays('2026-11-30', 1)).toBe('2026-12-01');
    expect(addDays('2026-03-01', -1)).toBe('2026-02-28');
    expect(addMonths('2026-12', 1)).toBe('2027-01');
    expect(addMonths('2026-01', -1)).toBe('2025-12');
  });
  it('formats a short day independent of the browser timezone', () => {
    expect(fmtShortDay('2026-11-18')).toBe('Wed 18 Nov');
  });
});

describe('native bridge', () => {
  afterEach(() => {
    delete window.NimragApp;
  });
  it('is unknown without the app', () => {
    expect(readNativeStatus()).toEqual({ link: 'unknown', lastSyncAt: null });
  });
  it('reads the app status and rejects malformed values', () => {
    window.NimragApp = { status: () => ({ link: 'waiting', lastSyncAt: '2026-11-19T07:42:00Z' }) };
    expect(readNativeStatus()).toEqual({ link: 'waiting', lastSyncAt: '2026-11-19T07:42:00Z' });
    window.NimragApp = { status: () => ({ link: 'bogus' as never, lastSyncAt: 5 as never }) };
    expect(readNativeStatus()).toEqual({ link: 'unknown', lastSyncAt: null });
    window.NimragApp = {
      status: () => {
        throw new Error('bridge down');
      },
    };
    expect(readNativeStatus().link).toBe('unknown');
  });
});

describe('login redirect target', () => {
  it('only allows same-origin paths', () => {
    expect(safeNext('/activities?type=run')).toBe('/activities?type=run');
    expect(safeNext('https://evil.example/')).toBe('/');
    expect(safeNext('//evil.example')).toBe('/');
    expect(safeNext('/login')).toBe('/');
    expect(safeNext(null)).toBe('/');
  });
});
