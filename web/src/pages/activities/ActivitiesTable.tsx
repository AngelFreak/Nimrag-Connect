import { Link } from 'react-router-dom';
import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import { ValueOrUnavailable } from '../../components';
import { fmtInt } from '../../lib/format';
import { activityTitle, fmtMinutes, rowText, whenLabel, type Sort, type SortKey } from './format';
import './activities.css';

export function activityHref(a: Pick<ActivityListItem, 'id' | 'kind'>): string {
  return a.kind === 'strength' ? `/activities/${a.id}/strength` : `/activities/${a.id}`;
}

interface Col {
  key: SortKey | 'detail';
  header: string;
  end?: boolean;
}

const COLS: Col[] = [
  { key: 'date', header: 'Date' },
  { key: 'type', header: 'Type' },
  { key: 'duration', header: 'Duration' },
  { key: 'detail', header: 'Detail' },
  { key: 'hr', header: 'Avg HR', end: true },
];

/** Desktop table of the loaded activities; the headers sort them (missing values last). */
export function ActivitiesTable({ rows, sort, onSort }: { rows: readonly ActivityListItem[]; sort: Sort; onSort: (key: SortKey) => void }) {
  return (
    // A scrollable region must be focusable so keyboard users can scroll it.
    // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
    <div className="table-wrap" role="region" aria-label="Activities (scrollable)" tabIndex={0}>
      <table className="table acts-table">
        <caption className="sr-only">Activities, newest first unless sorted otherwise</caption>
        <thead>
          <tr>
            {COLS.map((c) => {
              const sortable = c.key !== 'detail';
              const active = sortable && sort.key === c.key;
              return (
                <th
                  key={c.key}
                  scope="col"
                  className={c.end ? 'is-end' : undefined}
                  aria-sort={active ? (sort.dir === 'asc' ? 'ascending' : 'descending') : sortable ? 'none' : undefined}
                >
                  {sortable ? (
                    <button type="button" className="sort-btn" onClick={() => onSort(c.key as SortKey)}>
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
          {rows.map((a) => {
            const text = rowText(a);
            return (
              <tr key={a.id}>
                <td>
                  <Link to={activityHref(a)}>{whenLabel(a)}</Link>
                </td>
                <td>{activityTitle(a)}</td>
                <td>
                  <ValueOrUnavailable value={fmtMinutes(a.duration_s)} />
                </td>
                <td className="is-muted">
                  <ValueOrUnavailable value={text.detail} />
                </td>
                <td className="is-end">
                  <ValueOrUnavailable value={fmtInt(a.avg_hr_bpm)} />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
