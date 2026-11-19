import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { ValueOrUnavailable } from './Unavailable';

interface ListRowProps {
  to: string;
  title: ReactNode;
  sub: ReactNode;
  /** Right-hand value ("42 min"); null renders "unavailable". */
  value: ReactNode | null | undefined;
  valueSub?: ReactNode;
}

/** Activity list row: a link card with title and date on the left, key figure on the right. */
export function ListRow({ to, title, sub, value, valueSub }: ListRowProps) {
  return (
    <Link to={to} className="list-row">
      <span className="list-row__main">
        <span className="list-row__title">{title}</span>
        <span className="list-row__sub">{sub}</span>
      </span>
      <span className="list-row__end">
        <span className="list-row__value">
          <ValueOrUnavailable value={value} />
        </span>
        {valueSub ? <span className="list-row__sub">{valueSub}</span> : null}
      </span>
    </Link>
  );
}
