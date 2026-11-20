import { describe, expect, it } from 'vitest';
import { entryWhat, fmtChange, fmtEntryWhen, parseNumber, toPoints, validateEntry } from './body';
import { activityWords, dayMarkers, daysOfMonth, leadingBlanks, moveFocus, sleepText, energyText } from './calendar';
import { fmtKm, weekStepsSummary } from './movement';
import { localParts } from './time';
import { fmtRecoveryLeft, seriesSummary, sinceText, statusView, targetText, vsTargetText } from './training';
import { calendar, training } from './testUtils';

describe('local timestamps', () => {
  it('reads the owner-zone date and time from the text, never through the browser zone', () => {
    expect(localParts('2026-11-20T09:00:00+01:00')).toEqual({ day: '2026-11-20', time: '09:00' });
    expect(localParts('2026-11-20T23:59:00-08:00')).toEqual({ day: '2026-11-20', time: '23:59' });
    expect(localParts(null)).toBeNull();
    expect(localParts('yesterday')).toBeNull();
    expect(fmtEntryWhen('2026-11-17T07:10:00+01:00')).toBe('Tue 17 Nov · 07:10');
  });
});

describe('movement helpers', () => {
  it('km and week summary keep missing days as "no data", not 0', () => {
    expect(fmtKm(4600)).toBe('4.6 km');
    expect(fmtKm(null)).toBeNull();
    const s = weekStepsSummary(
      [
        { date: '2026-11-16', value: 9120 },
        { date: '2026-11-17', value: null },
      ],
      8000,
    );
    expect(s).toContain('Monday 9,120');
    expect(s).toContain('Tuesday no data');
    expect(s).toContain('8,000');
  });
});

describe('training helpers', () => {
  it('recovery time in hours, or minutes under an hour; missing stays missing', () => {
    expect(fmtRecoveryLeft(14 * 3600)).toBe('14 h');
    expect(fmtRecoveryLeft(35 * 60)).toBe('35 min');
    expect(fmtRecoveryLeft(null)).toBeNull();
  });
  it('words for the load against its target only come from the API value', () => {
    expect(vsTargetText('within')).toBe('Inside the target range');
    expect(vsTargetText('wild')).toBeNull();
    expect(vsTargetText(null)).toBeNull();
    expect(targetText(240, 420)).toBe('240 to 420');
    expect(targetText(240, null)).toBeNull();
  });
  it('training status never shows a raw code and invents no word for it', () => {
    const base = training().status;
    const other = { headline: 'Other status', note: 'The watch reports a status that has no name here yet.' };
    expect(statusView(base)).toEqual(other);
    expect(statusView({ ...base, label: 'Status 9' })).toEqual(other);
    expect(statusView({ ...base, label: 'Maintaining' }).headline).toBe('Maintaining');
    expect(statusView({ ...base, status_code: null })).toEqual({ headline: null, note: null });
    expect(sinceText({ ...base, since_is_lower_bound: true })).toBe('Since Thu 5 Nov or earlier');
    expect(sinceText({ ...base, since: null })).toBeNull();
  });
  it('summarises a series for screen readers', () => {
    const t = seriesSummary('Load', 'over 4 weeks', [
      { date: '2026-11-17', value: 300 },
      { date: '2026-11-18', value: null },
      { date: '2026-11-19', value: 320 },
    ]);
    expect(t).toContain('300 on Tue 17 Nov');
    expect(t).toContain('320 on Thu 19 Nov');
    expect(seriesSummary('Load', 'over 4 weeks', [])).toBe('Load over 4 weeks: no readings.');
  });
});

describe('body entry validation', () => {
  const today = '2026-11-19';
  it('parses decimals with a comma or a point and rejects other text', () => {
    expect(parseNumber('80,5')).toBe(80.5);
    expect(parseNumber(' 80.5 ')).toBe(80.5);
    expect(parseNumber('')).toBeNull();
    expect(parseNumber('abc')).toBeNaN();
    expect(parseNumber('-3')).toBeNaN();
  });
  it('needs at least one measurement', () => {
    const r = validateEntry({ date: today, weight: '', fat: '', waist: '' }, today);
    expect(r.payload).toBeNull();
    expect(r.errors.form).toMatch(/at least one/);
  });
  it('checks each value against the server limits', () => {
    const r = validateEntry({ date: today, weight: '10', fat: '90', waist: 'x' }, today);
    expect(r.payload).toBeNull();
    expect(r.errors.weight).toMatch(/between 20 and 400/);
    expect(r.errors.fat).toMatch(/between 2 and 70/);
    expect(r.errors.waist).toMatch(/number/);
    expect(r.errors.form).toBeUndefined();
  });
  it('rejects a future or missing date', () => {
    expect(validateEntry({ date: '2026-11-20', weight: '80', fat: '', waist: '' }, today).errors.date).toMatch(/future/);
    expect(validateEntry({ date: '', weight: '80', fat: '', waist: '' }, today).errors.date).toMatch(/Choose a date/);
  });
  it('builds the request: today leaves the time to the server, another day is sent as a date', () => {
    expect(validateEntry({ date: today, weight: '80,5', fat: '', waist: '86' }, today).payload).toEqual({ weight_kg: 80.5, waist_cm: 86 });
    expect(validateEntry({ date: '2026-11-17', weight: '', fat: '18.2', waist: '' }, today).payload).toEqual({ measured_at: '2026-11-17', body_fat_pct: 18.2 });
  });
  it('describes entries and the 30-day change', () => {
    expect(entryWhat({ id: 1, measured_at: '', weight_kg: 80.8, body_fat_pct: null, waist_cm: 86 })).toBe('Weight 80.8 kg · Waist 86.0 cm');
    expect(fmtChange(-1.4)).toBe('−1.4 kg');
    expect(fmtChange(0.6)).toBe('+0.6 kg');
    expect(fmtChange(null)).toBeNull();
  });
  it('keeps only readings that have a value, in time order', () => {
    const pts = toPoints([
      { time: '2026-11-17T07:10:00+01:00', value: 80 },
      { time: '2026-11-03T07:10:00+01:00', value: null },
      { time: '2026-10-27T07:10:00+01:00', value: 82 },
    ]);
    expect(pts.map((p) => p.v)).toEqual([82, 80]);
  });
});

describe('calendar helpers', () => {
  it('lays out November 2026 with Monday first', () => {
    expect(leadingBlanks('2026-11')).toBe(6); // 1 Nov 2026 is a Sunday
    expect(daysOfMonth('2026-02')).toHaveLength(28);
    expect(daysOfMonth('2028-02')).toHaveLength(29);
  });
  it('moves with arrows by a day or a week, Home and End to the week row', () => {
    expect(moveFocus('2026-11-19', 'ArrowLeft')).toBe('2026-11-18');
    expect(moveFocus('2026-11-19', 'ArrowRight')).toBe('2026-11-20');
    expect(moveFocus('2026-11-19', 'ArrowUp')).toBe('2026-11-12');
    expect(moveFocus('2026-11-19', 'ArrowDown')).toBe('2026-11-26');
    expect(moveFocus('2026-11-19', 'Home')).toBe('2026-11-16');
    expect(moveFocus('2026-11-19', 'End')).toBe('2026-11-22');
    expect(moveFocus('2026-11-30', 'ArrowRight')).toBe('2026-12-01');
    expect(moveFocus('2026-11-19', 'a')).toBeNull();
  });
  it('markers by shape: cardio, strength, or both; words for the accessible name', () => {
    const days = new Map(calendar().days.map((d) => [d.date, d]));
    expect(dayMarkers(days.get('2026-11-03'))).toEqual(['strength']);
    expect(dayMarkers(days.get('2026-11-05'))).toEqual(['cardio']);
    expect(dayMarkers(days.get('2026-11-10'))).toEqual(['cardio', 'strength']);
    expect(dayMarkers(days.get('2026-11-01'))).toEqual([]);
    expect(dayMarkers(undefined)).toEqual([]);
    expect(activityWords(days.get('2026-11-10'))).toBe('run, strength');
    expect(activityWords(days.get('2026-11-01'))).toBeNull();
  });
  it('sleep and energy text tolerate half-missing values', () => {
    const d = calendar().days[0]!;
    expect(sleepText(d)).toBeNull();
    expect(sleepText({ ...d, sleep_score: 81 })).toBe('score 81');
    expect(sleepText({ ...d, sleep_duration_s: 26520, sleep_score: 81 })).toBe('7h 22m · score 81');
    expect(energyText({ ...d, energy_high: 94 })).toBe('High 94');
    expect(energyText(d)).toBeNull();
  });
});
