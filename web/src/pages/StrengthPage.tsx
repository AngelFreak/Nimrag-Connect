import { useParams } from 'react-router-dom';
import { SampleDataChip, TopBar } from '../components';
import { useIsDesktop, usePageTitle } from '../lib/hooks';
import { useStrength } from './activities/api';
import { whenLabel } from './activities/format';
import { PageSkeleton, QueryError } from './activities/PageStates';
import { StrengthSession } from './activities/StrengthSession';

export function StrengthPage() {
  const { id } = useParams();
  usePageTitle('Strength session');
  const query = useStrength(id);
  const data = query.data;
  const desktop = useIsDesktop();

  // On a wide screen the session draws its own breadcrumb and title.
  if (desktop && query.isSuccess) return <StrengthSession data={query.data} />;

  return (
    <>
      <TopBar
        variant="back"
        backTo="/strength"
        backLabel="Back to strength sessions"
        title="Strength"
        subtitle={data ? whenLabel({ date: data.start.slice(0, 10), start: data.start }) : undefined}
        aside={<SampleDataChip sample={data?.sample ?? false} />}
      />
      {query.isPending ? (
        <PageSkeleton label="Loading strength session" tiles={3} />
      ) : query.isError ? (
        <QueryError error={query.error} what="Strength session" onRetry={() => void query.refetch()} backTo="/strength" />
      ) : (
        <StrengthSession data={query.data} />
      )}
    </>
  );
}
