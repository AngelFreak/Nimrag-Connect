import type { ReactNode } from 'react';
import { isMissing } from '../lib/format';

/**
 * Hard rule: a metric the watch or server could not provide is shown as the word
 * "unavailable", never as 0, a dash or an empty cell.
 */
export function Unavailable() {
  return <span className="unavailable">unavailable</span>;
}

/** Renders `value`, or <Unavailable /> when it is missing (null, undefined, NaN, empty string). */
export function ValueOrUnavailable({ value }: { value: ReactNode | null | undefined }) {
  if (isMissing(value) || value === '' || value === false) return <Unavailable />;
  return <>{value}</>;
}
