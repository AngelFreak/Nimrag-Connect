// The workout builder's model: what the form holds (text, as typed), how it becomes the body the
// server stores, and the limits it checks first. The server checks them again; the messages here
// say the same thing in the same words so a refusal from either side reads alike.

import type { DurationDto, IntensityDto, NodeDto, SportDto, StepDto, TargetDto, WorkoutBody, WorkoutLimits } from '../../api/generated';

export interface Choice<V extends string> {
  value: V;
  label: string;
}

export const SPORTS: readonly Choice<SportDto>[] = [
  { value: 'running', label: 'Running' },
  { value: 'cycling', label: 'Cycling' },
  { value: 'walking', label: 'Walking' },
  { value: 'hiking', label: 'Hiking' },
  { value: 'strength', label: 'Strength' },
  { value: 'other', label: 'Other' },
];

export const INTENSITIES: readonly Choice<IntensityDto>[] = [
  { value: 'warmup', label: 'Warm-up' },
  { value: 'active', label: 'Active' },
  { value: 'rest', label: 'Rest' },
  { value: 'recovery', label: 'Recovery' },
  { value: 'cooldown', label: 'Cool-down' },
];

export type DurationKind = DurationDto['kind'];
export type TargetKind = TargetDto['kind'];

export const DURATIONS: readonly Choice<DurationKind>[] = [
  { value: 'time', label: 'Time' },
  { value: 'distance', label: 'Distance' },
  { value: 'heart_rate_below', label: 'Until heart rate is below' },
  { value: 'heart_rate_above', label: 'Until heart rate is above' },
  { value: 'open', label: 'Open (until you press lap)' },
];

export const TARGETS: readonly Choice<TargetKind>[] = [
  { value: 'none', label: 'No target' },
  { value: 'heart_rate_zone', label: 'Heart rate zone' },
  { value: 'heart_rate_range', label: 'Heart rate range' },
  { value: 'pace', label: 'Pace range' },
  { value: 'speed', label: 'Speed range' },
  { value: 'cadence', label: 'Cadence range' },
];

/** The server's limits, used until the list says otherwise (and in tests). */
export const DEFAULT_LIMITS: WorkoutLimits = {
  max_workouts: 200,
  max_steps: 100,
  max_block_steps: 20,
  repeat_min: 2,
  repeat_max: 99,
  name_max_bytes: 63,
  note_max_bytes: 254,
};

// ---- the draft: every number is text until the form is saved ----

export interface StepDraft {
  key: string;
  intensity: IntensityDto;
  durationKind: DurationKind;
  /** Time "m:ss", metres, or bpm, by `durationKind`. */
  duration: string;
  targetKind: TargetKind;
  /** Zone, low or fast (pace); by `targetKind`. */
  targetA: string;
  /** High or slow (pace). */
  targetB: string;
  note: string;
}

export type NodeDraft = { type: 'step'; key: string; step: StepDraft } | { type: 'repeat'; key: string; count: string; steps: StepDraft[] };

export interface WorkoutDraft {
  name: string;
  sport: SportDto;
  note: string;
  nodes: NodeDraft[];
}

let counter = 0;
export function newKey(): string {
  counter += 1;
  return `k${counter}`;
}

export function newStep(over: Partial<StepDraft> = {}): StepDraft {
  return { key: newKey(), intensity: 'active', durationKind: 'time', duration: '5:00', targetKind: 'none', targetA: '', targetB: '', note: '', ...over };
}

export function newRepeat(): NodeDraft {
  return {
    type: 'repeat',
    key: newKey(),
    count: '4',
    steps: [newStep({ intensity: 'active', duration: '1:00' }), newStep({ intensity: 'recovery', duration: '1:00' })],
  };
}

export function emptyDraft(): WorkoutDraft {
  return { name: '', sport: 'running', note: '', nodes: [{ type: 'step', key: newKey(), step: newStep({ intensity: 'warmup', duration: '10:00' }) }] };
}

/** A copy of a step with a new key, placed after the original. */
export function copyStep(step: StepDraft): StepDraft {
  return { ...step, key: newKey() };
}

/** Moves the item at `from` to `to` (both inside the array); the same array when nothing moves. */
export function moved<T>(items: readonly T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= items.length || to >= items.length) return [...items];
  const next = [...items];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item as T);
  return next;
}

// ---- reading and writing the text fields ----

/** "5:00", "1:30:00" or a bare number of minutes ("5"); seconds, or null when it is not a time. */
export function parseClock(text: string): number | null {
  const t = text.trim();
  if (/^\d+$/.test(t)) return Number(t) * 60;
  const m = /^(\d+):([0-5]\d)$/.exec(t);
  if (m) return Number(m[1]) * 60 + Number(m[2]);
  const h = /^(\d+):([0-5]\d):([0-5]\d)$/.exec(t);
  if (h) return Number(h[1]) * 3600 + Number(h[2]) * 60 + Number(h[3]);
  return null;
}

/** Seconds as "m:ss", or "h:mm:ss" from one hour. */
export function clockText(seconds: number): string {
  const total = Math.round(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

function parseInteger(text: string): number | null {
  const t = text.trim();
  return /^\d{1,7}$/.test(t) ? Number(t) : null;
}

function parseDecimal(text: string): number | null {
  const t = text.trim().replace(',', '.');
  return /^\d{1,4}(\.\d{1,2})?$/.test(t) ? Number(t) : null;
}

function decimalText(n: number): string {
  return String(Number(n.toFixed(1)));
}

const byteLength = (s: string): number => new TextEncoder().encode(s).length;

// ---- the body <-> the draft ----

function stepDraftOf(step: StepDto): StepDraft {
  const draft = newStep({ intensity: step.intensity, note: step.note ?? '' });
  const d = step.duration;
  draft.durationKind = d.kind;
  draft.duration = d.kind === 'time' ? clockText(d.seconds) : d.kind === 'distance' ? String(d.meters) : d.kind === 'open' ? '' : String(d.bpm);
  const t = step.target;
  draft.targetKind = t.kind;
  switch (t.kind) {
    case 'none':
      break;
    case 'heart_rate_zone':
      draft.targetA = String(t.zone);
      break;
    case 'heart_rate_range':
      draft.targetA = String(t.low_bpm);
      draft.targetB = String(t.high_bpm);
      break;
    case 'pace':
      draft.targetA = clockText(t.fast_s_per_km);
      draft.targetB = clockText(t.slow_s_per_km);
      break;
    case 'speed':
      draft.targetA = decimalText(t.low_kmh);
      draft.targetB = decimalText(t.high_kmh);
      break;
    case 'cadence':
      draft.targetA = String(t.low);
      draft.targetB = String(t.high);
      break;
  }
  return draft;
}

export function draftOf(body: WorkoutBody): WorkoutDraft {
  return {
    name: body.name,
    sport: body.sport,
    note: body.note ?? '',
    nodes: body.nodes.map(
      (n): NodeDraft =>
        n.type === 'step'
          ? { type: 'step', key: newKey(), step: stepDraftOf(n) }
          : { type: 'repeat', key: newKey(), count: String(n.count), steps: n.steps.map(stepDraftOf) },
    ),
  };
}

/** Errors by field. `fields` keys are `<step or block key>:<field>`; `form` is about the whole workout. */
export interface Errors {
  name?: string;
  note?: string;
  form?: string;
  fields: Record<string, string>;
}

export const fieldKey = (key: string, field: 'duration' | 'target' | 'note' | 'count' | 'steps'): string => `${key}:${field}`;

/** Steps in the file: every step plus one for each repeat block (what the watch counts against its limit). */
export function fileSteps(nodes: readonly NodeDraft[]): number {
  return nodes.reduce((sum, n) => sum + (n.type === 'step' ? 1 : n.steps.length + 1), 0);
}

/** Steps the owner sees: every step, those in repeat blocks included. */
export function stepCount(nodes: readonly NodeDraft[]): number {
  return nodes.reduce((sum, n) => sum + (n.type === 'step' ? 1 : n.steps.length), 0);
}

interface StepResult {
  step: StepDto | null;
  errors: Record<string, string>;
}

function stepOf(d: StepDraft, limits: WorkoutLimits): StepResult {
  const errors: Record<string, string> = {};
  let duration: DurationDto | null = null;
  switch (d.durationKind) {
    case 'open':
      duration = { kind: 'open' };
      break;
    case 'time': {
      const s = parseClock(d.duration);
      if (s === null) errors[fieldKey(d.key, 'duration')] = 'Enter a time like 5:00 (minutes and seconds).';
      else if (s < 1 || s > 86_400) errors[fieldKey(d.key, 'duration')] = 'A step lasts 1 second to 24 hours.';
      else duration = { kind: 'time', seconds: s };
      break;
    }
    case 'distance': {
      const m = parseInteger(d.duration);
      if (m === null) errors[fieldKey(d.key, 'duration')] = 'Enter a distance in whole metres, like 400.';
      else if (m < 1 || m > 500_000) errors[fieldKey(d.key, 'duration')] = 'A step covers 1 metre to 500 km.';
      else duration = { kind: 'distance', meters: m };
      break;
    }
    case 'heart_rate_below':
    case 'heart_rate_above': {
      const bpm = parseInteger(d.duration);
      if (bpm === null || bpm < 30 || bpm > 240) errors[fieldKey(d.key, 'duration')] = 'A heart rate is 30 to 240 bpm.';
      else duration = { kind: d.durationKind, bpm };
      break;
    }
  }

  let target: TargetDto | null = null;
  const bad = (text: string) => {
    errors[fieldKey(d.key, 'target')] = text;
  };
  switch (d.targetKind) {
    case 'none':
      target = { kind: 'none' };
      break;
    case 'heart_rate_zone': {
      const zone = parseInteger(d.targetA);
      if (zone === null || zone < 1 || zone > 5) bad('A heart rate zone is 1 to 5.');
      else target = { kind: 'heart_rate_zone', zone };
      break;
    }
    case 'heart_rate_range': {
      const low = parseInteger(d.targetA);
      const high = parseInteger(d.targetB);
      if (low === null || high === null || low < 30 || high > 240 || low > high || high < 30 || low > 240) bad('A heart rate range is 30 to 240 bpm, low not above high.');
      else target = { kind: 'heart_rate_range', low_bpm: low, high_bpm: high };
      break;
    }
    case 'pace': {
      const fast = parseClock(d.targetA);
      const slow = parseClock(d.targetB);
      if (fast === null || slow === null) bad('Enter both paces like 5:30 (minutes and seconds per km).');
      else if (fast < 60 || slow > 3600 || fast > slow || slow < 60 || fast > 3600) bad('A pace range is 1:00 to 60:00 per km, fast not slower than slow.');
      else target = { kind: 'pace', fast_s_per_km: fast, slow_s_per_km: slow };
      break;
    }
    case 'speed': {
      const low = parseDecimal(d.targetA);
      const high = parseDecimal(d.targetB);
      if (low === null || high === null || low < 0.5 || high > 100 || low > high || high < 0.5 || low > 100) bad('A speed range is 0.5 to 100 km/h, low not above high.');
      else target = { kind: 'speed', low_kmh: low, high_kmh: high };
      break;
    }
    case 'cadence': {
      const low = parseInteger(d.targetA);
      const high = parseInteger(d.targetB);
      if (low === null || high === null || low < 1 || high > 300 || low > high || high < 1 || low > 300) bad('A cadence range is 1 to 300, low not above high.');
      else target = { kind: 'cadence', low, high };
      break;
    }
  }

  const note = d.note.trim();
  if (byteLength(note) > limits.note_max_bytes) errors[fieldKey(d.key, 'note')] = `A step note is at most ${limits.note_max_bytes} bytes.`;

  if (duration === null || target === null || Object.keys(errors).length > 0) return { step: null, errors };
  return { step: { intensity: d.intensity, duration, target, ...(note ? { note } : {}) }, errors };
}

export interface Validated {
  errors: Errors;
  /** The body to save, or null while anything is wrong. */
  body: WorkoutBody | null;
}

/** Checks the draft against the limits and, when it holds, builds the body the server stores. */
export function validate(draft: WorkoutDraft, limits: WorkoutLimits = DEFAULT_LIMITS): Validated {
  const errors: Errors = { fields: {} };
  const name = draft.name.trim();
  if (name === '') errors.name = 'Give the workout a name.';
  else if (byteLength(name) > limits.name_max_bytes) errors.name = `The name is at most ${limits.name_max_bytes} bytes (about ${limits.name_max_bytes} plain letters).`;
  const note = draft.note.trim();
  if (byteLength(note) > limits.note_max_bytes) errors.note = `The note is at most ${limits.note_max_bytes} bytes.`;

  if (draft.nodes.length === 0) errors.form = 'The workout has no steps. Add one.';
  else if (fileSteps(draft.nodes) > limits.max_steps) {
    errors.form = `A workout has at most ${limits.max_steps} steps; a repeat block counts one more than the steps in it.`;
  }

  const nodes: NodeDto[] = [];
  let complete = true;
  for (const node of draft.nodes) {
    if (node.type === 'step') {
      const r = stepOf(node.step, limits);
      Object.assign(errors.fields, r.errors);
      if (r.step) nodes.push({ type: 'step', ...r.step });
      else complete = false;
      continue;
    }
    const count = parseInteger(node.count);
    if (count === null || count < limits.repeat_min || count > limits.repeat_max) {
      errors.fields[fieldKey(node.key, 'count')] = `A repeat runs ${limits.repeat_min} to ${limits.repeat_max} times.`;
      complete = false;
    }
    if (node.steps.length === 0 || node.steps.length > limits.max_block_steps) {
      errors.fields[fieldKey(node.key, 'steps')] = `A repeat block holds 1 to ${limits.max_block_steps} steps.`;
      complete = false;
    }
    const inner: StepDto[] = [];
    for (const s of node.steps) {
      const r = stepOf(s, limits);
      Object.assign(errors.fields, r.errors);
      if (r.step) inner.push(r.step);
      else complete = false;
    }
    if (count !== null && complete) nodes.push({ type: 'repeat', count, steps: inner });
  }

  const hasErrors = errors.name || errors.note || errors.form || Object.keys(errors.fields).length > 0;
  if (hasErrors || !complete) return { errors, body: null };
  return { errors, body: { name, sport: draft.sport, ...(note ? { note } : {}), nodes } };
}

// ---- reading a workout back as words ----

export function sportLabel(sport: SportDto): string {
  return SPORTS.find((s) => s.value === sport)?.label ?? sport;
}

export function intensityLabel(i: IntensityDto): string {
  return INTENSITIES.find((x) => x.value === i)?.label ?? i;
}

export function durationText(d: DurationDto): string {
  switch (d.kind) {
    case 'open':
      return 'until lap';
    case 'time':
      return clockText(d.seconds);
    case 'distance':
      return d.meters >= 1000 && d.meters % 100 === 0 ? `${decimalText(d.meters / 1000)} km` : `${d.meters} m`;
    case 'heart_rate_below':
      return `until HR below ${d.bpm}`;
    case 'heart_rate_above':
      return `until HR above ${d.bpm}`;
  }
}

export function targetText(t: TargetDto): string | null {
  switch (t.kind) {
    case 'none':
      return null;
    case 'heart_rate_zone':
      return `zone ${t.zone}`;
    case 'heart_rate_range':
      return `${t.low_bpm} to ${t.high_bpm} bpm`;
    case 'pace':
      return `${clockText(t.fast_s_per_km)} to ${clockText(t.slow_s_per_km)} /km`;
    case 'speed':
      return `${decimalText(t.low_kmh)} to ${decimalText(t.high_kmh)} km/h`;
    case 'cadence':
      return `cadence ${t.low} to ${t.high}`;
  }
}

/** "Warm-up, 10:00, zone 2". */
export function stepText(s: StepDto): string {
  return [intensityLabel(s.intensity), durationText(s.duration), targetText(s.target)].filter(Boolean).join(', ');
}

/** Total time of the steps, repeats counted; null when any step has no time (distance, heart rate, open). */
export function totalSeconds(nodes: readonly NodeDto[]): number | null {
  let total = 0;
  for (const n of nodes) {
    const steps = n.type === 'step' ? [n] : n.steps;
    const times = steps.map((s) => (s.duration.kind === 'time' ? s.duration.seconds : null));
    if (times.some((t) => t === null)) return null;
    const sum = times.reduce<number>((a, t) => a + (t ?? 0), 0);
    total += n.type === 'step' ? sum : sum * n.count;
  }
  return total;
}

/** "45 min" / "1 h 05 min" for the list; null when the workout has a step without a time. */
export function totalText(seconds: number | null): string | null {
  if (seconds === null) return null;
  const minutes = Math.max(1, Math.round(seconds / 60));
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, '0')} min`;
}
