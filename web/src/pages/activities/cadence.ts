// Cadence as runners read it. On foot the watch stores strides a minute (one foot), which is half of
// what Garmin Connect and every running guide call cadence: steps a minute, both feet. So on-foot
// activities are doubled once, when they arrive, and shown in spm; other sports stay as stored (rpm).

import type { ActivityDetail } from '../../api/generated';

const ON_FOOT = new Set(['run', 'walk', 'hike']);

/** Whether this kind of activity counts cadence in steps (both feet). */
export function stepsCadence(kind: string): boolean {
  return ON_FOOT.has(kind);
}

/** The unit after a cadence figure: "spm" on foot, "rpm" otherwise. */
export function cadenceUnit(kind: string): 'spm' | 'rpm' {
  return stepsCadence(kind) ? 'spm' : 'rpm';
}

const double = (v: number | null): number | null => (v === null ? null : v * 2);

/** The activity with its cadence in the unit it is shown in (doubled on foot). */
export function withShownCadence(a: ActivityDetail): ActivityDetail {
  if (!stepsCadence(a.kind)) return a;
  return {
    ...a,
    avg_cadence: double(a.avg_cadence),
    series: a.series ? { ...a.series, cadence: a.series.cadence.map(double) } : a.series,
  };
}
