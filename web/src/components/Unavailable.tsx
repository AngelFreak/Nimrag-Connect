import type { ReactNode } from 'react';
import { isMissing } from '../lib/format';

/**
 * Hard rule: a metric the watch or server could not provide is never shown as 0. Inside a card
 * with other content the figure is left out; a card with nothing says so in one sentence
 * (<EmptyNote />). The word "unavailable" stays only where leaving a figure out would misalign a table.
 */
export function Unavailable() {
  return <span className="unavailable">unavailable</span>;
}

/** Renders `value`, or <Unavailable /> when it is missing (null, undefined, NaN, empty string). */
export function ValueOrUnavailable({ value }: { value: ReactNode | null | undefined }) {
  if (isMissing(value) || value === '' || value === false) return <Unavailable />;
  return <>{value}</>;
}

/**
 * A card or section where nothing is known: one short sentence that says why or what to do
 * ("No weight logged yet."). Use it instead of a list of "unavailable".
 */
export function EmptyNote({ children }: { children: ReactNode }) {
  return <p className="empty-note">{children}</p>;
}
