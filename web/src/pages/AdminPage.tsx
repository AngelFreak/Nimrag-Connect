import { useMe } from '../account/queries';
import { TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { UnassignedCard } from './admin/UnassignedCard';
import { UsersCard } from './admin/UsersCard';

/**
 * The admin area (admin sessions only): accounts, and the data nobody owns yet. An admin has no
 * data of their own, so this is the only page they see.
 */
export function AdminPage() {
  usePageTitle('Admin');
  const me = useMe();
  const timezone = me.data?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone;

  return (
    <>
      <TopBar variant="large" title="Admin" subtitle="Users and unassigned data" />
      <div className="page">
        <div className="grid-panels" style={{ alignItems: 'start' }}>
          <UsersCard />
          <UnassignedCard timezone={timezone} />
        </div>
      </div>
    </>
  );
}
