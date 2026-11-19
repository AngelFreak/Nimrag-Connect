import { useParams } from 'react-router-dom';
import { SampleDataChip, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { useActivity } from './activities/api';
import { ActivityBody } from './activities/ActivityBody';
import { activityTitle, whenLabel } from './activities/format';
import { PageSkeleton, QueryError } from './activities/PageStates';

export function ActivityDetailPage() {
  const { id } = useParams();
  const query = useActivity(id);
  const a = query.data;
  const title = a ? activityTitle(a) : 'Activity';
  usePageTitle(title);
  const backTo = a?.kind === 'strength' ? '/activities?type=strength' : '/activities';

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
      {query.isPending ? (
        <PageSkeleton label="Loading activity" />
      ) : query.isError ? (
        <QueryError error={query.error} what="Activity" onRetry={() => void query.refetch()} backTo="/activities" />
      ) : (
        <ActivityBody a={query.data} />
      )}
    </>
  );
}
