import type { ReactNode } from 'react';
import { Card } from './Card';
import { ValueOrUnavailable } from './Unavailable';

export type Tone = 'energy' | 'sleep' | 'heart' | 'steps' | 'warn' | 'link' | 'muted';
export type TileSize = 'sm' | 'md' | 'lg' | 'xl';

interface StatTileProps {
  label: ReactNode;
  /** Pre-formatted value with its unit ("7h 12m"). null / undefined renders "unavailable". */
  value: ReactNode | null | undefined;
  /** Small line under the value, coloured by the metric (energy, sleep, ...). */
  sub?: ReactNode;
  tone?: Tone;
  size?: TileSize;
  /** Extra content under the sub line (e.g. a ProgressBar). */
  children?: ReactNode;
}

const PADDING = { sm: 'sm', md: 'tile', lg: 'lg', xl: 'xl' } as const;

/** Label, big value in Space Grotesk, and a sub line in the metric's colour. */
export function StatTile({ label, value, sub, tone = 'muted', size = 'md', children }: StatTileProps) {
  return (
    <Card padding={PADDING[size]} className={`stat stat--${size}`}>
      <span className="stat__label">{label}</span>
      <span className="stat__value">
        <ValueOrUnavailable value={value} />
      </span>
      {sub ? <span className={`stat__sub tone-${tone}`}>{sub}</span> : null}
      {children}
    </Card>
  );
}
