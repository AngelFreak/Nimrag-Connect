import { useParams } from 'react-router-dom';
import { SampleDataChip, TopBar } from '../components';
import { useIsDesktop, usePageTitle } from '../lib/hooks';
import { useActivity } from './activities/api';
import { ActivityBody } from './activities/ActivityBody';
import { ActivityDetailDesktop } from './activities/ActivityDetailDesktop';
import { activityTitle, whenLabel } from './activities/format';
import { PageSkeleton, QueryError } from './activities/PageStates';

export function ActivityDetailPage() {
  const { id } = useParams();
  const query = useActivity(id);
  const desktop = useIsDesktop();
  const a = query.data;
  const title = a ? activityTitle(a) : 'Activity';
  usePageTitle(title);
  const backTo = a?.kind === 'strength' ? '/strength' : '/activities';

  const body = query.isPending ? (
    <PageSkeleton label="Loading activity" />
  ) : query.isError ? (
    <QueryError error={query.error} what="Activity" onRetry={() => void query.refetch()} backTo="/activities" />
  ) : desktop ? (
    <ActivityDetailDesktop a={query.data} />
  ) : (
    <ActivityBody a={query.data} />
  );

  // On a wide screen the page draws its own breadcrumb and title (a loading or failed page has none, so it keeps a bare title).
  if (desktop && a && !query.isError) return body;

  return (
    <>
      <TopBar
        variant="back"
        backTo={backTo}
        backLabel="Back to activities"
        title={title}
        subtitle={a ? whenLabel(a) : undefined}
        aside={<SampleDataChip sample={a?.sample ?? false} />}
      />
      {body}
    </>
  );
}
