import type { MovementResponse } from '../../api/generated';
import { fmtDay } from '../../lib/dates';
import { fmtFixed, fmtInt, isMissing } from '../../lib/format';
import type { DateValue } from './types';

/** "Mon" for a local day. */
export function weekdayShort(day: string): string {
  return fmtDay(day, { weekday: 'short' });
}

export function weekdayLong(day: string): string {
  return fmtDay(day, { weekday: 'long' });
}

/** Kilometres with one decimal, "4.6 km"; null without a distance. */
export function fmtKm(distanceM: number | null | undefined): string | null {
  const km = isMissing(distanceM) ? null : fmtFixed(distanceM / 1000, 1);
  return km === null ? null : `${km} km`;
}

export function fmtKcal(kcal: number | null | undefined): string | null {
  const v = fmtInt(kcal);
  return v === null ? null : `${v} kcal`;
}

/** Text summary of the steps-per-day chart, read by screen readers. */
export function weekStepsSummary(week: DateValue[], goal: number): string {
  const parts = week.map((d) => {
    const v = fmtInt(d.value);
    return `${weekdayLong(d.date)} ${v ?? 'no data'}`;
  });
  return `Steps per day this week: ${parts.join(', ')}. The goal line is ${fmtInt(goal) ?? goal}.`;
}

/** Percent of the goal as a whole number, from the API's one-decimal value; null stays null. */
export function wholePercent(p: number | null | undefined): number | null {
  return isMissing(p) ? null : Math.round(p);
}

export interface IntensityParts {
  total: number | null;
  moderate: number | null;
  /** Vigorous minutes as recorded (they count double in the total). */
  vigorous: number | null;
}

export function intensityParts(m: MovementResponse): IntensityParts {
  const i = m.intensity;
  return {
    total: isMissing(i.total_min) ? null : i.total_min,
    moderate: isMissing(i.moderate_min) ? null : i.moderate_min,
    vigorous: isMissing(i.vigorous_min) ? null : i.vigorous_min,
  };
}
