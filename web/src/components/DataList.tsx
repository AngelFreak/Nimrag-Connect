import type { ReactNode } from 'react';
import { isMissing } from '../lib/format';
import { ValueOrUnavailable } from './Unavailable';
import type { Tone } from './StatTile';

export interface DataItem {
  label: ReactNode;
  /** null / undefined renders "unavailable". */
  value: ReactNode | null | undefined;
  tone?: Tone;
  strong?: boolean;
  /** Leave this row out when it has no value. */
  optional?: boolean;
}

interface DataListProps {
  items: DataItem[];
  /** "wide" puts labels in a narrow first column (Sync and storage on the desktop dashboard). */
  layout?: 'split' | 'wide';
  /** Leave out the rows that have no value (instead of a row that only says "unavailable"). */
  hideMissing?: boolean;
  /** Shown instead of the list when `hideMissing` left no row. */
  empty?: ReactNode;
}

/** Label and value pairs as a real description list (dt/dd). */
export function DataList({ items, layout = 'split', hideMissing = false, empty = null }: DataListProps) {
  const hasValue = (it: DataItem) => !isMissing(it.value) && it.value !== '' && it.value !== false;
  const shown = items.filter((it) => hasValue(it) || !(hideMissing || it.optional));
  if (shown.length === 0) return hideMissing && empty !== null ? <p className="data-list-empty">{empty}</p> : null;
  return (
    <dl className={layout === 'wide' ? 'data-list data-list--wide' : 'data-list'}>
      {shown.map((it, i) => (
        <div key={i} style={{ display: 'contents' }}>
          <dt className={it.strong ? 'is-strong' : undefined}>{it.label}</dt>
          <dd className={[it.tone ? `tone-${it.tone}` : '', it.strong ? 'is-strong' : ''].filter(Boolean).join(' ') || undefined}>
            <ValueOrUnavailable value={it.value} />
          </dd>
        </div>
      ))}
    </dl>
  );
}
