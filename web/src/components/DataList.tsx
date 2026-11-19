import type { ReactNode } from 'react';
import { ValueOrUnavailable } from './Unavailable';
import type { Tone } from './StatTile';

export interface DataItem {
  label: ReactNode;
  /** null / undefined renders "unavailable". */
  value: ReactNode | null | undefined;
  tone?: Tone;
  strong?: boolean;
}

interface DataListProps {
  items: DataItem[];
  /** "wide" puts labels in a narrow first column (Sync and storage on the desktop dashboard). */
  layout?: 'split' | 'wide';
}

/** Label and value pairs as a real description list (dt/dd). */
export function DataList({ items, layout = 'split' }: DataListProps) {
  return (
    <dl className={layout === 'wide' ? 'data-list data-list--wide' : 'data-list'}>
      {items.map((it, i) => (
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
