// Strength session logic without UI: drafts, validation, the PATCH body and the comparison.

import type { ExerciseGroup } from '../../api/generated/ExerciseGroup';
import type { SetPatch } from '../../api/generated/SetPatch';
import type { StrengthSet } from '../../api/generated/StrengthSet';
import { isMissing } from '../../lib/format';
import { fmtKg } from './format';

// ---- drafts ---------------------------------------------------------------------------------

/** What the inputs hold while editing: text, so half-typed values are kept as typed. */
export interface SetDraft {
  reps: string;
  weight: string;
  exercise: string;
}
export type Drafts = Record<number, SetDraft>;

function numText(v: number | null): string {
  return v === null ? '' : String(Math.round(v * 100) / 100);
}

export function draftOf(s: StrengthSet): SetDraft {
  return { reps: numText(s.reps), weight: numText(s.weight_kg), exercise: s.exercise ?? '' };
}

/** The values the watch recorded. */
export function watchDraftOf(s: StrengthSet): SetDraft {
  return { reps: numText(s.original_reps), weight: numText(s.original_weight_kg), exercise: s.original_exercise ?? '' };
}

export function draftsOf(groups: readonly ExerciseGroup[]): Drafts {
  const out: Drafts = {};
  for (const g of groups) for (const s of g.sets) out[s.idx] = draftOf(s);
  return out;
}

export const MAX_REPS = 10_000;
export const MAX_WEIGHT = 2_000;
export const MAX_NAME = 80;

type Parsed = { ok: true; value: number | null } | { ok: false; error: string };

/** Empty clears the value (unknown reps); otherwise a whole number from 0 to 10,000. */
export function parseReps(text: string): Parsed {
  const t = text.trim();
  if (t === '') return { ok: true, value: null };
  if (!/^\d+$/.test(t) || Number(t) > MAX_REPS) return { ok: false, error: 'Reps: a whole number from 0 to 10,000, or leave empty.' };
  return { ok: true, value: Number(t) };
}

/** Empty clears the value (bodyweight); otherwise 0 to 2,000 kg, comma or dot as the decimal mark. */
export function parseWeight(text: string): Parsed {
  const t = text.trim().replace(',', '.');
  if (t === '') return { ok: true, value: null };
  if (!/^\d+(\.\d*)?$/.test(t) || Number(t) > MAX_WEIGHT) return { ok: false, error: 'Weight: a number from 0 to 2,000 kg, or leave empty for bodyweight.' };
  return { ok: true, value: Number(t) };
}

export interface SetErrors {
  reps?: string;
  weight?: string;
  exercise?: string;
}

export function validateSet(s: StrengthSet, d: SetDraft): SetErrors {
  const e: SetErrors = {};
  const reps = parseReps(d.reps);
  if (!reps.ok) e.reps = reps.error;
  const weight = parseWeight(d.weight);
  if (!weight.ok) e.weight = weight.error;
  const name = d.exercise.trim();
  // An unchanged name is always fine, even the empty one of an exercise the watch could not name.
  if (name !== (s.exercise ?? '') && (name === '' || [...name].length > MAX_NAME)) e.exercise = 'Exercise: 1 to 80 characters.';
  return e;
}

export function hasErrors(e: SetErrors): boolean {
  return !!(e.reps || e.weight || e.exercise);
}

/** Errors per set idx, only for sets that have any. */
export function validateAll(groups: readonly ExerciseGroup[], drafts: Drafts): Record<number, SetErrors> {
  const out: Record<number, SetErrors> = {};
  for (const g of groups) {
    for (const s of g.sets) {
      const d = drafts[s.idx];
      if (!d) continue;
      const e = validateSet(s, d);
      if (hasErrors(e)) out[s.idx] = e;
    }
  }
  return out;
}

/** True when the draft differs from what the watch recorded (or the set carries a saved edit). */
export function differsFromWatch(s: StrengthSet, d: SetDraft): boolean {
  if (s.edited) return true;
  const reps = parseReps(d.reps);
  const weight = parseWeight(d.weight);
  if (!reps.ok || !weight.ok) return true;
  return reps.value !== s.original_reps || weight.value !== s.original_weight_kg || d.exercise.trim() !== (s.original_exercise ?? '');
}

/**
 * The PATCH body for the changed sets: only changed fields, `null` for a cleared value,
 * `reset: true` when every value is back to the watch's and the set still carries an edit.
 * Call only when `validateAll` found no errors.
 */
export function buildPatch(groups: readonly ExerciseGroup[], drafts: Drafts): SetPatch[] {
  const out: SetPatch[] = [];
  for (const g of groups) {
    for (const s of g.sets) {
      const d = drafts[s.idx];
      if (!d) continue;
      const reps = parseReps(d.reps);
      const weight = parseWeight(d.weight);
      if (!reps.ok || !weight.ok) continue;
      const name = d.exercise.trim();
      const repsChanged = reps.value !== s.reps;
      const weightChanged = weight.value !== s.weight_kg;
      const nameChanged = name !== (s.exercise ?? '');
      if (!repsChanged && !weightChanged && !nameChanged) continue;
      const backToWatch = reps.value === s.original_reps && weight.value === s.original_weight_kg && name === (s.original_exercise ?? '');
      if (backToWatch && s.edited) {
        out.push({ idx: s.idx, reset: true });
        continue;
      }
      const p: SetPatch = { idx: s.idx };
      if (repsChanged) p.reps = reps.value;
      if (weightChanged) p.weight_kg = weight.value;
      if (nameChanged) p.exercise = name;
      out.push(p);
    }
  }
  return out;
}

// ---- view text --------------------------------------------------------------------------------

/** "10, 10, 10 reps", "60 s each", "10, unavailable, 8 reps"; `null` without any reps or time. */
export function repsText(sets: readonly StrengthSet[]): string | null {
  if (sets.length === 0) return null;
  if (sets.every((s) => s.reps === null)) {
    const secs = sets.map((s) => s.duration_s);
    if (secs.every((v) => v === null)) return null;
    if (secs.every((v) => v === secs[0])) return `${Math.round(secs[0] as number)} s each`;
    return `${secs.map((v) => (v === null ? 'unavailable' : Math.round(v))).join(', ')} s`;
  }
  return `${sets.map((s) => (s.reps === null ? 'unavailable' : s.reps)).join(', ')} reps`;
}

/** "22 kg", "20 to 24 kg"; `null` for bodyweight (no weight on any set). */
export function loadText(sets: readonly StrengthSet[]): string | null {
  const w = sets.map((s) => s.weight_kg).filter((v): v is number => !isMissing(v));
  if (w.length === 0) return null;
  const lo = Math.min(...w);
  const hi = Math.max(...w);
  return lo === hi ? fmtKg(lo) : `${fmtKg(lo)?.replace(' kg', '')} to ${fmtKg(hi)}`;
}

// ---- comparison with the previous session ----------------------------------------------------------

export interface ExerciseTotals {
  name: string;
  sets: number;
  reps: number | null;
  topWeight: number | null;
  volume: number | null;
}

/** One line per exercise name (a name can come back in several groups). Order of first appearance. */
export function exerciseTotals(groups: readonly ExerciseGroup[]): ExerciseTotals[] {
  const by = new Map<string, ExerciseTotals>();
  for (const g of groups) {
    const name = g.name ?? 'Unnamed exercise';
    const key = name.trim().toLowerCase();
    const t = by.get(key) ?? { name, sets: 0, reps: null, topWeight: null, volume: null };
    for (const s of g.sets) {
      t.sets += 1;
      if (s.reps !== null) t.reps = (t.reps ?? 0) + s.reps;
      if (s.weight_kg !== null) t.topWeight = Math.max(t.topWeight ?? 0, s.weight_kg);
      if (s.reps !== null && s.weight_kg !== null) t.volume = (t.volume ?? 0) + s.reps * s.weight_kg;
    }
    by.set(key, t);
  }
  return [...by.values()];
}

export interface ExerciseComparison {
  name: string;
  now: ExerciseTotals;
  before: ExerciseTotals | null;
}

export interface Comparison {
  rows: ExerciseComparison[];
  /** Exercises in the previous session that this one does not have. */
  onlyBefore: ExerciseTotals[];
}

export function compareExercises(now: readonly ExerciseGroup[], before: readonly ExerciseGroup[]): Comparison {
  const prev = exerciseTotals(before);
  const find = (name: string) => prev.find((p) => p.name.trim().toLowerCase() === name.trim().toLowerCase()) ?? null;
  const cur = exerciseTotals(now);
  const rows = cur.map((c) => ({ name: c.name, now: c, before: find(c.name) }));
  const used = new Set(rows.map((r) => r.before?.name.trim().toLowerCase()));
  return { rows, onlyBefore: prev.filter((p) => !used.has(p.name.trim().toLowerCase())) };
}

/** "+2", "-1", "same"; `null` when either side is missing. Sign and word, never colour alone. */
export function deltaText(now: number | null, before: number | null, unit = '', digits = 0): string | null {
  if (now === null || before === null) return null;
  const d = Math.round((now - before) * 10 ** digits) / 10 ** digits;
  if (d === 0) return 'same';
  const n = Math.abs(d).toLocaleString('en-GB', { maximumFractionDigits: digits });
  return `${d > 0 ? '+' : '-'}${n}${unit}`;
}
