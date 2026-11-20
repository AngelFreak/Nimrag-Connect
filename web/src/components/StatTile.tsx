import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { Card } from './Card';
import { isMissing } from '../lib/format';
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
  /** Render nothing when `value` is missing (a tile that would only say "unavailable" is noise). */
  hideWhenMissing?: boolean;
  /** Makes the whole tile a link to this route ("/sleep"). Keep `children` non-interactive then. */
  href?: string;
  /** `data-*` attributes go on the link when there is one, otherwise on the card. */
  [dataAttribute: `data-${string}`]: string | undefined;
}

const PADDING = { sm: 'sm', md: 'tile', lg: 'lg', xl: 'xl' } as const;

/** Label, big value in Space Grotesk, and a sub line in the metric's colour. */
export function StatTile({ label, value, sub, tone = 'muted', size = 'md', children, href, hideWhenMissing = false, ...rest }: StatTileProps) {
  if (hideWhenMissing && (isMissing(value) || value === '' || value === false)) return null;
  const tile = (
    <Card padding={PADDING[size]} className={`stat stat--${size}`} {...(href ? {} : rest)}>
      <span className="stat__label">{label}</span>
      <span className="stat__value">
        <ValueOrUnavailable value={value} />
      </span>
      {sub ? <span className={`stat__sub tone-${tone}`}>{sub}</span> : null}
      {children}
    </Card>
  );
  return href ? (
    <Link to={href} className="stat-link" {...rest}>
      {tile}
    </Link>
  ) : (
    tile
  );
}
