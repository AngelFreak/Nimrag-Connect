// The heart rate zone of a lap. The API does not send the zone limits, so they are read from the
// activity itself: the share of time the watch put in each zone says which heart rate values the
// zone limits must lie between.

import type { ZoneTime } from '../../api/generated/ZoneTime';
import { isMissing } from '../../lib/format';
import { quantile } from './routeColor';

/** Fewer samples than this say too little about the limits. */
const MIN_SAMPLES = 20;

/**
 * Lower limit of zones 1 to 5, in bpm: `limits[0]` is where zone 1 starts and `limits[4]` where zone 5 starts.
 * `null` when the file has too few samples or no time in zones.
 */
export function zoneLimits(heartRate: readonly (number | null)[], zones: readonly ZoneTime[], belowZone1_s: number | null): number[] | null {
  const bpm = heartRate.filter((v): v is number => !isMissing(v)).sort((a, b) => a - b);
  if (bpm.length < MIN_SAMPLES) return null;
  const secs = [1, 2, 3, 4, 5].map((z) => zones.find((x) => x.zone === z)?.duration_s ?? null);
  if (secs.some((s) => s === null)) return null;
  const below = belowZone1_s ?? 0;
  const total = below + (secs as number[]).reduce((a, b) => a + b, 0);
  if (!(total > 0)) return null;
  let cum = below;
  const limits: number[] = [];
  for (let z = 0; z < 5; z += 1) {
    limits.push(quantile(bpm, Math.min(1, cum / total)));
    cum += secs[z] as number;
  }
  return limits;
}

/** The zone (0 = below zone 1, 1 to 5) a heart rate falls in. */
export function zoneOf(bpm: number, limits: readonly number[]): number {
  let zone = 0;
  limits.forEach((limit, i) => {
    if (bpm >= limit) zone = i + 1;
  });
  return zone;
}
