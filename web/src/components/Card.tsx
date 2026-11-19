import type { ElementType, ReactNode } from 'react';

export type CardPadding = 'default' | 'sm' | 'tile' | 'lg' | 'xl' | 'flush' | 'list';

interface CardProps {
  as?: ElementType;
  padding?: CardPadding;
  /** Lay the content out as a row (text on the left, a figure or action on the right). */
  row?: boolean;
  className?: string;
  'aria-label'?: string;
  'aria-labelledby'?: string;
  /** `data-*` attributes (test hooks, CSS hooks). */
  [dataAttribute: `data-${string}`]: string | undefined;
  children: ReactNode;
}

/** The 16 px radius surface every block of content sits on. */
export function Card({ as, padding = 'default', row, className, children, ...rest }: CardProps) {
  const Tag = as ?? 'section';
  const cls = ['card', padding !== 'default' && `card--${padding}`, row && 'card--row', className].filter(Boolean).join(' ');
  return (
    <Tag className={cls} {...rest}>
      {children}
    </Tag>
  );
}

interface CardHeaderProps {
  title: ReactNode;
  /** Right-hand summary, e.g. "Avg 61 ms · range 55 to 70". */
  aside?: ReactNode;
  level?: 2 | 3;
  tone?: 'default' | 'muted' | 'lg';
  id?: string;
}

export function CardHeader({ title, aside, level = 2, tone = 'default', id }: CardHeaderProps) {
  const Heading = level === 2 ? 'h2' : 'h3';
  const cls = ['card__title', tone === 'muted' && 'card__title--muted', tone === 'lg' && 'card__title--lg'].filter(Boolean).join(' ');
  return (
    <div className="card__head">
      <Heading className={cls} id={id}>
        {title}
      </Heading>
      {aside ? <span className="card__aside">{aside}</span> : null}
    </div>
  );
}
