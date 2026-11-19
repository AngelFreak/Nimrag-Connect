import { Link } from 'react-router-dom';
import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import { Table, type Column } from '../../components';
import { fmtInt } from '../../lib/format';
import { activityTitle, fmtMinutes, rowText, whenLabel, type Sort, type SortKey } from './format';
import './activities.css';

export function activityHref(a: Pick<ActivityListItem, 'id' | 'kind'>): string {
  return a.kind === 'strength' ? `/activities/${a.id}/strength` : `/activities/${a.id}`;
}

const COLUMNS: Column<ActivityListItem>[] = [
  { key: 'date', header: 'Date', sortable: true, cell: (a) => <Link to={activityHref(a)}>{whenLabel(a)}</Link> },
  { key: 'type', header: 'Type', sortable: true, cell: (a) => activityTitle(a) },
  { key: 'duration', header: 'Duration', sortable: true, cell: (a) => fmtMinutes(a.duration_s) },
  { key: 'detail', header: 'Detail', muted: true, cell: (a) => rowText(a).detail },
  { key: 'hr', header: 'Avg HR', sortable: true, align: 'end', cell: (a) => fmtInt(a.avg_hr_bpm) },
];

/** Desktop table of the loaded activities; the headers sort them (missing values last). */
export function ActivitiesTable({ rows, sort, onSort }: { rows: readonly ActivityListItem[]; sort: Sort; onSort: (key: SortKey) => void }) {
  return (
    <Table
      className="acts-table"
      caption="Activities, newest first unless sorted otherwise"
      columns={COLUMNS}
      rows={rows}
      rowKey={(a) => a.id}
      sort={sort}
      onSort={(key) => onSort(key as SortKey)}
    />
  );
}
