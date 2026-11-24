// Status colours in one place: every status word (training status, HRV status, sleep rating,
// readiness level, stress level, energy level) gets its dot colour from here. The colours are the
// tokens in styles/tokens.css, so both themes follow. A colour never stands alone: the word is shown.

import type { StatusKind } from '../api/generated/StatusKind';
import { LEVEL_COLOR as STRESS_COLOR } from './stress';

export const STATUS_COLOR: Record<StatusKind, string> = {
  recovery: 'var(--status-recovery)',
  strained: 'var(--status-strained)',
  maintaining: 'var(--status-maintaining)',
  productive: 'var(--status-productive)',
  detraining: 'var(--status-detraining)',
  none: 'var(--status-none)',
  unknown: 'var(--status-none)',
};

const HRV_COLOR = {
  balanced: 'var(--hrv-balanced)',
  unbalanced: 'var(--hrv-unbalanced)',
  low: 'var(--hrv-low)',
  poor: 'var(--hrv-low)',
};

const SLEEP_COLOR = {
  excellent: 'var(--rating-excellent)',
  good: 'var(--rating-good)',
  fair: 'var(--rating-fair)',
  poor: 'var(--rating-poor)',
};

/** Readiness levels, worst to best (the five rungs `--rate-1` to `--rate-5`). */
const READINESS_COLOR = {
  poor: 'var(--rate-1)',
  low: 'var(--rate-2)',
  moderate: 'var(--rate-3)',
  high: 'var(--rate-4)',
  prime: 'var(--rate-5)',
};

/** Energy reserve now: below 30 low, 30 to 59 moderate, 60 and up good (the words of the Today card). */
const ENERGY_COLOR = {
  low: 'var(--rate-2)',
  moderate: 'var(--rate-3)',
  good: 'var(--rate-4)',
};

/**
 * Overnight values against your own usual range (Health status). Outside the range is not "bad",
 * only different, so both directions use the same amber; the word says which way.
 */
const HEALTH_COLOR = {
  in_range: 'var(--status-productive)',
  above: 'var(--warn)',
  below: 'var(--warn)',
  calibrating: 'var(--status-none)',
  no_reading: 'var(--status-none)',
};

export type StatusType = 'training' | 'hrv' | 'sleep' | 'readiness' | 'stress' | 'energy' | 'health';

const MAPS: Record<StatusType, Record<string, string>> = {
  training: STATUS_COLOR,
  hrv: HRV_COLOR,
  sleep: SLEEP_COLOR,
  readiness: READINESS_COLOR,
  stress: STRESS_COLOR,
  energy: ENERGY_COLOR,
  health: HEALTH_COLOR,
};

/** The dot colour of a status, or `null` for a key nothing knows (no dot is drawn then). Case is ignored. */
export function statusColor(type: StatusType, key: string | null | undefined): string | null {
  if (!key) return null;
  return MAPS[type][key.toLowerCase()] ?? null;
}

/** The energy key of the same thresholds as "Good reserve", "Moderate reserve" and "Low reserve". */
export function energyKey(now: number | null | undefined): 'low' | 'moderate' | 'good' | null {
  if (now === null || now === undefined || !Number.isFinite(now)) return null;
  return now >= 60 ? 'good' : now >= 30 ? 'moderate' : 'low';
}
