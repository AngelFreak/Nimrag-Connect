// Display helpers. Missing values (null, undefined, NaN) always return `null`, never "0":
// callers pass the result to components that render "unavailable" for null (hard rule 3).

export type Maybe<T> = T | null | undefined;

export function isMissing(value: unknown): value is null | undefined {
  return value === null || value === undefined || (typeof value === 'number' && !Number.isFinite(value));
}

const intFmt = new Intl.NumberFormat('en-GB', { maximumFractionDigits: 0 });

export function fmtInt(value: Maybe<number>): string | null {
  return isMissing(value) ? null : intFmt.format(value);
}

export function fmtFixed(value: Maybe<number>, digits: number): string | null {
  return isMissing(value) ? null : value.toFixed(digits);
}

/** Minutes as "7h 12m" (or "12m" under an hour). */
export function fmtDurationMin(minutes: Maybe<number>): string | null {
  if (isMissing(minutes)) return null;
  const total = Math.round(minutes);
  const h = Math.floor(total / 60);
  const m = total % 60;
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

/** Seconds as "38:00" or "1:02:03". */
export function fmtClock(seconds: Maybe<number>): string | null {
  if (isMissing(seconds)) return null;
  const total = Math.round(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** Joins a number and its unit, "62 ms"; `null` when the number is missing. */
export function withUnit(value: string | null, unit: string): string | null {
  return value === null ? null : `${value} ${unit}`;
}
