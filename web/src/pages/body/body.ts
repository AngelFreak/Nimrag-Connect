import type { BodyEntry, NewBodyEntry } from '../../api/generated';
import { fmtShortDay, isIsoDay } from '../../lib/dates';
import { fmtFixed, isMissing } from '../../lib/format';
import { localParts } from './time';
import type { TimedValue } from './types';

/** The limits the server enforces (server/src/web_api/body.rs); checked here first for a quick answer. */
export const LIMITS = {
  weight: { min: 20, max: 400, label: 'Weight', unit: 'kg' },
  fat: { min: 2, max: 70, label: 'Body fat', unit: '%' },
  waist: { min: 30, max: 300, label: 'Waist', unit: 'cm' },
} as const;

export interface EntryForm {
  date: string;
  weight: string;
  fat: string;
  waist: string;
}

export interface FormErrors {
  date?: string;
  weight?: string;
  fat?: string;
  waist?: string;
  /** Nothing was filled in. */
  form?: string;
}

/** "80,5" and "80.5" both mean 80.5; empty means "not given" (null); anything else is NaN. */
export function parseNumber(text: string): number | null {
  const t = text.trim().replace(',', '.');
  if (t === '') return null;
  if (!/^\d+(\.\d+)?$/.test(t)) return Number.NaN;
  return Number(t);
}

function check(text: string, spec: { min: number; max: number; label: string; unit: string }): { value: number | null; error?: string } {
  const v = parseNumber(text);
  if (v === null) return { value: null };
  if (Number.isNaN(v)) return { value: null, error: `${spec.label} must be a number, for example 80.5.` };
  if (v < spec.min || v > spec.max) return { value: null, error: `${spec.label} must be between ${spec.min} and ${spec.max} ${spec.unit}.` };
  return { value: v };
}

export interface Validated {
  errors: FormErrors;
  /** Present only when there are no errors. */
  payload: NewBodyEntry | null;
}

/**
 * Checks the add-entry form. A date equal to `today` is left out of the request so the server
 * stamps the entry with the current time; any other date is sent as YYYY-MM-DD (noon that day).
 */
export function validateEntry(f: EntryForm, today: string): Validated {
  const errors: FormErrors = {};
  const w = check(f.weight, LIMITS.weight);
  const b = check(f.fat, LIMITS.fat);
  const c = check(f.waist, LIMITS.waist);
  if (w.error) errors.weight = w.error;
  if (b.error) errors.fat = b.error;
  if (c.error) errors.waist = c.error;
  if (!isIsoDay(f.date)) errors.date = 'Choose a date.';
  else if (f.date > today) errors.date = 'The date cannot be in the future.';
  if (w.value === null && b.value === null && c.value === null && !errors.weight && !errors.fat && !errors.waist) {
    errors.form = 'Fill in at least one of weight, body fat or waist.';
  }
  if (Object.keys(errors).length > 0) return { errors, payload: null };
  const payload: NewBodyEntry = {};
  if (f.date !== today) payload.measured_at = f.date;
  if (w.value !== null) payload.weight_kg = w.value;
  if (b.value !== null) payload.body_fat_pct = b.value;
  if (c.value !== null) payload.waist_cm = c.value;
  return { errors, payload };
}

/** "Tue 17 Nov · 07:10" from the entry's own timestamp (owner's timezone, never reinterpreted). */
export function fmtEntryWhen(measuredAt: string): string {
  const p = localParts(measuredAt);
  return p ? `${fmtShortDay(p.day)} · ${p.time}` : measuredAt;
}

/** "Weight 80.8 kg · Waist 86.0 cm": the values the entry holds, in words. */
export function entryWhat(e: BodyEntry): string {
  const parts: string[] = [];
  if (!isMissing(e.weight_kg)) parts.push(`Weight ${fmtFixed(e.weight_kg, 1)} kg`);
  if (!isMissing(e.body_fat_pct)) parts.push(`Body fat ${fmtFixed(e.body_fat_pct, 1)} %`);
  if (!isMissing(e.waist_cm)) parts.push(`Waist ${fmtFixed(e.waist_cm, 1)} cm`);
  return parts.join(' · ');
}

export function fmtChange(kg: number | null | undefined): string | null {
  if (isMissing(kg)) return null;
  const abs = Math.abs(kg).toFixed(1);
  if (abs === '0.0') return '0.0 kg';
  return `${kg < 0 ? '−' : '+'}${abs} kg`;
}

export interface WeightPoint {
  /** Epoch milliseconds. */
  t: number;
  v: number;
  /** Local day of the reading, from the timestamp text. */
  day: string;
}

/** Readings with a value, in time order. */
export function toPoints(series: TimedValue[]): WeightPoint[] {
  const out: WeightPoint[] = [];
  for (const p of series) {
    const t = Date.parse(p.time);
    const day = localParts(p.time)?.day;
    if (typeof p.value === 'number' && Number.isFinite(p.value) && Number.isFinite(t) && day) out.push({ t, v: p.value, day });
  }
  return out.sort((a, b) => a.t - b.t);
}

/** Text summary of the weight chart for screen readers. */
export function weightSummary(weights: WeightPoint[], trend: WeightPoint[]): string {
  if (weights.length === 0) return 'No weigh-ins in the last 90 days.';
  const first = weights[0]!;
  const last = weights[weights.length - 1]!;
  const lo = Math.min(...weights.map((p) => p.v));
  const hi = Math.max(...weights.map((p) => p.v));
  const tr = trend.length > 0 ? ` The smoothed trend ends at ${trend[trend.length - 1]!.v.toFixed(1)} kg.` : '';
  return `Weight over the last 90 days: ${weights.length} weigh-ins, from ${first.v.toFixed(1)} kg on ${fmtShortDay(first.day)} to ${last.v.toFixed(1)} kg on ${fmtShortDay(last.day)}, lowest ${lo.toFixed(1)}, highest ${hi.toFixed(1)}.${tr}`;
}
