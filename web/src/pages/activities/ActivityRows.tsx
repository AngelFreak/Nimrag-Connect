import { Link } from 'react-router-dom';
import type { ActivityListItem } from '../../api/generated/ActivityListItem';
import { SearchIcon, ValueOrUnavailable, useGroupArrows } from '../../components';
import { fmtMonthDay } from '../../lib/dates';
import { activityTitle, listStats, TYPE_OPTIONS, weekdayClock, whenLabel, type TypeKey } from './format';
import { SportDisc, sportIcon } from './SportDisc';
import './activities.css';

export function activityHref(a: Pick<ActivityListItem, 'id' | 'kind'>): string {
  return a.kind === 'strength' ? `/activities/${a.id}/strength` : `/activities/${a.id}`;
}

/** The name field of the desktop toolbar: filters the loaded rows as you type. */
export function ActivitySearch({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="acts-search">
      <input
        type="search"
        className="acts-search__input"
        aria-label="Search loaded activities"
        placeholder="Search activities"
        autoComplete="off"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
      <SearchIcon size={18} />
    </div>
  );
}

/** The sport filter as one icon button per type (same values and URL as the chips of the phone layout). */
export function TypeFilter({ value, onChange }: { value: TypeKey; onChange: (v: TypeKey) => void }) {
  const ref = useGroupArrows<HTMLDivElement>();
  return (
    <div ref={ref} role="group" aria-label="Filter by type" className="acts-types">
      {TYPE_OPTIONS.map((o) => (
        <button key={o.value} type="button" aria-pressed={o.value === value} aria-label={o.label} title={o.label} onClick={() => onChange(o.value)}>
          {sportIcon(o.value, 20)}
        </button>
      ))}
    </div>
  );
}

/** "Nov 23" over the year and the clock. */
function DateCell({ a }: { a: ActivityListItem }) {
  return (
    <span className="act-row__date" aria-hidden="true">
      <span className="act-row__day">{fmtMonthDay(a.date)}</span>
      <span className="act-row__year">{a.date.slice(0, 4)}</span>
    </span>
  );
}

/**
 * The activities as cards in a list: sport disc, date, the name (a real link that stretches over the
 * whole card) and five figures. Each figure is a term and its value, so a screen reader hears
 * "Distance 7.04 km"; a missing one says "unavailable".
 */
export function ActivityRows({ rows, label }: { rows: readonly ActivityListItem[]; label: string }) {
  return (
    <ul className="act-rows" aria-label={label}>
      {rows.map((a) => (
        <li key={a.id} className="act-row">
          <SportDisc kind={a.kind} />
          <DateCell a={a} />
          <div className="act-row__name">
            <Link to={activityHref(a)} className="act-row__link">
              {activityTitle(a)}
              <span className="sr-only">, {whenLabel(a)}</span>
            </Link>
            <span className="act-row__sub">{weekdayClock(a)}</span>
          </div>
          <dl className="act-row__stats">
            {listStats(a).map((s) => (
              <div key={s.label} className="act-row__stat">
                <dt>{s.label}</dt>
                <dd>
                  <ValueOrUnavailable value={s.value} />
                </dd>
              </div>
            ))}
          </dl>
        </li>
      ))}
    </ul>
  );
}
