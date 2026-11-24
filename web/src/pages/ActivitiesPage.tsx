import { useMemo, useState } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import { Button, EmptyState, FilterChips, LinkButton, ListRow, PageHeader, SampleDataChip, TopBar } from '../components';
import { browserToday } from '../lib/dates';
import { useIsDesktop, usePageTitle } from '../lib/hooks';
import { ActivityRows, ActivitySearch, activityHref, TypeFilter } from './activities/ActivityRows';
import { useActivityList } from './activities/api';
import {
  activityTitle,
  groupByPeriod,
  parseType,
  rowText,
  searchActivities,
  sortActivities,
  SORT_CHOICES,
  sortChoiceId,
  TYPE_OPTIONS,
  whenLabel,
  type Sort,
  type TypeKey,
} from './activities/format';
import { errorText, QueryError, RowsSkeleton } from './activities/PageStates';
import './activities/activities.css';

/**
 * The activities list. At `/strength` (`strength` set) it is the strength sessions: the path the
 * Strength tab uses in the browser and in the Android app, so both report the same route.
 */
export function ActivitiesPage({ strength = false }: { strength?: boolean }) {
  usePageTitle(strength ? 'Strength' : 'Activities');
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
  const type = strength ? 'strength' : parseType(params.get('type'));
  const desktop = useIsDesktop();
  const query = useActivityList(type);
  const [sort, setSort] = useState<Sort>({ key: 'date', dir: 'desc' });

  const items = useMemo(() => query.data?.pages.flatMap((p) => p.items) ?? [], [query.data]);
  const sample = query.data?.pages[0]?.sample ?? false;
  const groups = useMemo(() => groupByPeriod(items, browserToday()), [items]);
  const [search, setSearch] = useState('');
  const sorted = useMemo(() => searchActivities(sortActivities(items, sort), search), [items, sort, search]);

  const onType = (v: TypeKey) => {
    // Strength has its own path; every other type is a filter of /activities.
    if (v === 'strength') void navigate('/strength', { replace: true });
    else if (strength) void navigate(v === 'all' ? '/activities' : `/activities?type=${v}`, { replace: true });
    else setParams(v === 'all' ? {} : { type: v }, { replace: true });
  };

  let body;
  if (query.isPending) {
    body = <RowsSkeleton />;
  } else if (query.isError && items.length === 0) {
    body = <QueryError error={query.error} what="Activities" onRetry={() => void query.refetch()} backTo="/" />;
  } else if (items.length === 0) {
    body = (
      <div className="page" style={{ paddingTop: 0 }}>
        <EmptyState title={type === 'all' ? 'No activities yet' : 'No activities of this type'}>
          {type === 'all' ? 'Sync the watch to bring in its activities.' : 'Pick another type, or sync the watch to bring in new activities.'}
        </EmptyState>
      </div>
    );
  } else {
    body = (
      <div className={desktop ? 'acts-list' : 'page'} style={desktop ? undefined : { paddingTop: 0 }}>
        {desktop ? (
          <>
            {sorted.length > 0 ? (
              <ActivityRows rows={sorted} label={strength ? 'Strength sessions' : 'Activities'} />
            ) : (
              <p className="list-hint" role="status">
                No loaded activity matches “{search.trim()}”.{query.hasNextPage ? ' Load more to search further back.' : ''}
              </p>
            )}
            {query.hasNextPage && (search.trim() || sort.key !== 'date' || sort.dir !== 'desc') ? (
              <p className="list-hint">Search and sorting apply to the {items.length} activities loaded so far.</p>
            ) : null}
          </>
        ) : (
          groups.map((g) => (
            <section key={g.label} aria-label={g.label} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <h2 className="section-label">{g.label}</h2>
              {g.items.map((a) => {
                const text = rowText(a);
                return <ListRow key={a.id} to={activityHref(a)} title={activityTitle(a)} sub={whenLabel(a)} value={text.value} valueSub={text.sub} />;
              })}
            </section>
          ))
        )}
        <div className="load-more">
          {query.isFetchNextPageError ? (
            <p className="status-msg status-msg--error" role="alert">
              Could not load more activities. {errorText(query.error)}
            </p>
          ) : null}
          {query.hasNextPage ? (
            <Button onClick={() => void query.fetchNextPage()} disabled={query.isFetchingNextPage} aria-busy={query.isFetchingNextPage}>
              {query.isFetchingNextPage ? 'Loading more' : query.isFetchNextPageError ? 'Try again' : 'Load more'}
            </Button>
          ) : (
            <p className="list-hint">That is every activity: {items.length}.</p>
          )}
        </div>
      </div>
    );
  }

  if (desktop) {
    return (
      <>
        <PageHeader
          title={strength ? 'Strength' : 'Activities'}
          actions={
            <>
              <SampleDataChip sample={sample} />
              <nav className="acts-actions" aria-label="Activity actions">
                {strength ? <Link to="/workouts">Workouts</Link> : <Link to="/records">Records</Link>}
                <Link to="/workouts/new">+ Workout</Link>
                <Link to="/profile/import-garmin">Import</Link>
                <a href="/api/export" download title="Download all your data as a zip">
                  Export data
                </a>
              </nav>
            </>
          }
        />
        <div className="acts-toolbar">
          <ActivitySearch value={search} onChange={setSearch} />
          <TypeFilter value={type} onChange={onType} />
          <label className="acts-sort">
            <span className="sr-only">Sort loaded activities</span>
            <select value={sortChoiceId(sort)} onChange={(e) => setSort((SORT_CHOICES.find((c) => c.id === e.target.value) ?? SORT_CHOICES[0]!).sort)}>
              {SORT_CHOICES.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
        </div>
        {body}
      </>
    );
  }

  return (
    <>
      <TopBar
        variant="large"
        title={strength ? 'Strength' : 'Activities'}
        aside={
          <>
            <SampleDataChip sample={sample} />
            {strength ? <LinkButton to="/workouts">Workouts</LinkButton> : <LinkButton to="/records">Records</LinkButton>}
          </>
        }
      />
      <FilterChips label="Filter by type" options={TYPE_OPTIONS} value={type} onChange={onType} />
      {body}
    </>
  );
}
