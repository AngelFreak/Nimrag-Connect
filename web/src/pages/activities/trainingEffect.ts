// Training effect (0.0 to 5.0) in words, and the fill of the five-step bar.

import { isMissing, type Maybe } from '../../lib/format';

export const TE_MAX = 5;

/** The word for a training effect. The value is read as it is shown (one decimal): 0.95 shows as 1.0 and reads "Minor". */
export function teWord(value: number): string {
  const v = Math.round(value * 10) / 10;
  if (v < 1) return 'No effect';
  if (v < 2) return 'Minor';
  if (v < 3) return 'Maintaining';
  if (v < 4) return 'Improving';
  if (v < 5) return 'Highly improving';
  return 'Overreaching';
}

/** How full each of the five steps is (0 to 1): a value of 3.4 fills three steps and 0.4 of the fourth. */
export function teFill(value: number): number[] {
  const v = Math.min(TE_MAX, Math.max(0, Math.round(value * 10) / 10));
  return [0, 1, 2, 3, 4].map((i) => Math.min(1, Math.max(0, Math.round((v - i) * 10) / 10)));
}

export interface TeRow {
  key: 'aerobic' | 'anaerobic';
  label: string;
  value: number;
  /** "3.2" */
  text: string;
  word: string;
}

/** The effects that have a value, aerobic first. */
export function teRows(a: { aerobic_te: Maybe<number>; anaerobic_te: Maybe<number> }): TeRow[] {
  const row = (key: TeRow['key'], label: string, v: Maybe<number>): TeRow[] => (isMissing(v) ? [] : [{ key, label, value: v, text: v.toFixed(1), word: teWord(v) }]);
  return [...row('aerobic', 'Aerobic', a.aerobic_te), ...row('anaerobic', 'Anaerobic', a.anaerobic_te)];
}
