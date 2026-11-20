import type { TrainingResponse } from '../../api/generated';
import { fmtShortDay } from '../../lib/dates';
import { fmtFixed, fmtInt, isMissing } from '../../lib/format';
import type { DateValue } from './types';

/** Seconds left as "14 h" or "35 min"; "0 h" is shown as "Recovered" by the caller. */
export function fmtRecoveryLeft(seconds: number | null | undefined): string | null {
  if (isMissing(seconds)) return null;
  const minutes = Math.round(seconds / 60);
  if (minutes >= 60) return `${Math.round(minutes / 60)} h`;
  return `${minutes} min`;
}

const VS_TARGET: Record<string, string> = {
  below: 'Below the target range',
  within: 'Inside the target range',
  above: 'Above the target range',
};

/** Words for where the load sits against its target; null when the API does not say. */
export function vsTargetText(v: string | null | undefined): string | null {
  return v ? (VS_TARGET[v] ?? null) : null;
}

export function targetText(min: number | null | undefined, max: number | null | undefined): string | null {
  const a = fmtInt(min);
  const b = fmtInt(max);
  return a === null || b === null ? null : `${a} to ${b}`;
}

/** Values of a dated series, nulls kept (gaps are never drawn as 0). */
export function seriesValues(series: DateValue[]): (number | null)[] {
  return series.map((d) => d.value);
}

/** One sentence for a chart: first and last value with date, lowest and highest. */
export function seriesSummary(name: string, span: string, series: DateValue[], digits = 0): string {
  const known = series.filter((d): d is { date: string; value: number } => typeof d.value === 'number' && Number.isFinite(d.value));
  const f = (v: number) => (digits === 0 ? (fmtInt(v) ?? '') : (fmtFixed(v, digits) ?? ''));
  if (known.length === 0) return `${name} ${span}: no readings.`;
  const first = known[0]!;
  const last = known[known.length - 1]!;
  const lo = Math.min(...known.map((d) => d.value));
  const hi = Math.max(...known.map((d) => d.value));
  return `${name} ${span}: ${f(first.value)} on ${fmtShortDay(first.date)}, ${f(last.value)} on ${fmtShortDay(last.date)}, lowest ${f(lo)}, highest ${f(hi)}.`;
}

export interface StatusView {
  /** Headline: the watch's own label, or the numeric state when no label exists. */
  headline: string | null;
  /** Said under the headline when only a code exists. */
  note: string | null;
}

/**
 * The server names the status (label); a code nobody has verified comes as "Status N", so the
 * number fallback below only covers a response without a label.
 */
export function statusView(s: TrainingResponse['status']): StatusView {
  if (s.label) return { headline: s.label, note: null };
  if (s.status_code !== null && s.status_code !== undefined) {
    return { headline: `Status code ${s.status_code}`, note: 'The watch reports a number; its name is not known yet.' };
  }
  return { headline: null, note: null };
}

export function sinceText(s: TrainingResponse['status']): string | null {
  if (!s.since) return null;
  const day = fmtShortDay(s.since.slice(0, 10));
  return s.since_is_lower_bound ? `Since ${day} or earlier` : `Since ${day}`;
}
