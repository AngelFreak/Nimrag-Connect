import { useMemo, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { Button, Card, EmptyState, FilterChips, ListRow, SampleDataChip, TopBar } from '../components';
import { browserToday } from '../lib/dates';
import { useIsDesktop, usePageTitle } from '../lib/hooks';
import { ActivitiesTable, activityHref } from './activities/ActivitiesTable';
import { useActivityList } from './activities/api';
import { activityTitle, groupByPeriod, parseType, rowText, sortActivities, TYPE_OPTIONS, whenLabel, type Sort, type SortKey } from './activities/format';
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
  const sorted = useMemo(() => sortActivities(items, sort), [items, sort]);

  const onSort = (key: SortKey) =>
    setSort((s) => (s.key === key ? { key, dir: s.dir === 'asc' ? 'desc' : 'asc' } : { key, dir: key === 'type' ? 'asc' : 'desc' }));

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
      <div className="page" style={{ paddingTop: 0 }}>
        {desktop ? (
          <Card padding="xl">
            <ActivitiesTable rows={sorted} sort={sort} onSort={onSort} />
            {query.hasNextPage ? <p className="list-hint">Sorting applies to the {items.length} activities loaded so far.</p> : null}
          </Card>
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

  return (
    <>
      <TopBar variant="large" title={strength ? 'Strength' : 'Activities'} aside={<SampleDataChip sample={sample} />} />
      <FilterChips
        label="Filter by type"
        options={TYPE_OPTIONS}
        value={type}
        onChange={(v) => {
          // Strength has its own path; every other type is a filter of /activities.
          if (v === 'strength') void navigate('/strength', { replace: true });
          else if (strength) void navigate(v === 'all' ? '/activities' : `/activities?type=${v}`, { replace: true });
          else setParams(v === 'all' ? {} : { type: v }, { replace: true });
        }}
      />
      {body}
    </>
  );
}
