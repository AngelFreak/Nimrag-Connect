import type { ReactNode } from 'react';
import { ValueOrUnavailable } from './Unavailable';

export interface Column<Row> {
  key: string;
  header: ReactNode;
  /** Cell content; null / undefined renders "unavailable". */
  cell: (row: Row) => ReactNode | null | undefined;
  align?: 'start' | 'end';
  muted?: boolean;
  /** Name of the column in the stacked layout of a `wide` table; defaults to `header` when that is text. Leave empty for an actions column. */
  label?: string;
  /** The header becomes a button that calls `onSort(key)`; needs the table's `sort` and `onSort`. */
  sortable?: boolean;
}

export interface TableSort {
  key: string;
  dir: 'asc' | 'desc';
}

interface TableProps<Row> {
  columns: readonly Column<Row>[];
  rows: readonly Row[];
  rowKey: (row: Row, index: number) => string;
  /** Visible or screen-reader caption. */
  caption?: string;
  captionVisible?: boolean;
  density?: 'comfortable' | 'compact';
  /**
   * A table with several columns: in a box narrower than 560 px every row becomes a block (first
   * cell as its title, "label value" lines for the rest) instead of scrolling sideways.
   */
  wide?: boolean;
  /** Extra class on the `<table>` element (page-local column widths and links). */
  className?: string;
  /** Current sort; sortable headers show it with `aria-sort` and an arrow. Sorting itself is the caller's. */
  sort?: TableSort;
  onSort?: (key: string) => void;
}

/** Data table with real header cells; wide tables scroll inside a keyboard-focusable region. */
export function Table<Row>({ columns, rows, rowKey, caption, captionVisible, density = 'comfortable', wide, className, sort, onSort }: TableProps<Row>) {
  const cls = ['table', density === 'compact' && 'table--compact', wide && 'table--wide', className].filter(Boolean).join(' ');
  return (
    // A scrollable region must be focusable so keyboard users can scroll it (axe: scrollable-region-focusable).
    // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
    <div className="table-wrap" role="region" aria-label={caption ? `${caption} (scrollable)` : 'Table (scrollable)'} tabIndex={0}>
      <table className={cls}>
        {caption ? <caption className={captionVisible ? undefined : 'sr-only'}>{caption}</caption> : null}
        <thead>
          <tr>
            {columns.map((c) => {
              const sortable = c.sortable === true && onSort !== undefined;
              const active = sortable && sort?.key === c.key;
              return (
                <th
                  key={c.key}
                  scope="col"
                  className={[c.align === 'end' ? 'is-end' : '', sortable ? 'is-sortable' : ''].filter(Boolean).join(' ') || undefined}
                  aria-sort={sortable ? (active ? (sort.dir === 'asc' ? 'ascending' : 'descending') : 'none') : undefined}
                >
                  {sortable ? (
                    <button type="button" className="sort-btn" onClick={() => onSort(c.key)}>
                      {c.header}
                      <span aria-hidden="true" className="sort-btn__arrow">
                        {active ? (sort.dir === 'asc' ? '↑' : '↓') : ''}
                      </span>
                    </button>
                  ) : (
                    c.header
                  )}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={rowKey(row, i)}>
              {columns.map((c) => (
                <td
                  key={c.key}
                  data-label={c.label ?? (typeof c.header === 'string' ? c.header : undefined)}
                  className={[c.align === 'end' ? 'is-end' : '', c.muted ? 'is-muted' : ''].filter(Boolean).join(' ') || undefined}
                >
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
