import type { ReactNode } from 'react';
import { ValueOrUnavailable } from './Unavailable';

export interface Column<Row> {
  key: string;
  header: ReactNode;
  /** Cell content; null / undefined renders "unavailable". */
  cell: (row: Row) => ReactNode | null | undefined;
  align?: 'start' | 'end';
  muted?: boolean;
}

interface TableProps<Row> {
  columns: readonly Column<Row>[];
  rows: readonly Row[];
  rowKey: (row: Row, index: number) => string;
  /** Visible or screen-reader caption. */
  caption?: string;
  captionVisible?: boolean;
  density?: 'comfortable' | 'compact';
  /** Force horizontal scrolling below this table width (desktop tables on a phone). */
  wide?: boolean;
}

/** Data table with real header cells; wide tables scroll inside a keyboard-focusable region. */
export function Table<Row>({ columns, rows, rowKey, caption, captionVisible, density = 'comfortable', wide }: TableProps<Row>) {
  const cls = ['table', density === 'compact' && 'table--compact', wide && 'table--wide'].filter(Boolean).join(' ');
  return (
    // A scrollable region must be focusable so keyboard users can scroll it (axe: scrollable-region-focusable).
    // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
    <div className="table-wrap" role="region" aria-label={caption ? `${caption} (scrollable)` : 'Table (scrollable)'} tabIndex={0}>
      <table className={cls}>
        {caption ? <caption className={captionVisible ? undefined : 'sr-only'}>{caption}</caption> : null}
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c.key} scope="col" className={c.align === 'end' ? 'is-end' : undefined}>
                {c.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={rowKey(row, i)}>
              {columns.map((c) => (
                <td key={c.key} className={[c.align === 'end' ? 'is-end' : '', c.muted ? 'is-muted' : ''].filter(Boolean).join(' ') || undefined}>
                  <ValueOrUnavailable value={c.cell(row)} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
