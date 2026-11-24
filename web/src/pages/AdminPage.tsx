import { useMe } from '../account/queries';
import { TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { BackupsCard } from './admin/BackupsCard';
import { UnassignedCard } from './admin/UnassignedCard';
import { UsersCard } from './admin/UsersCard';

/**
 * The admin area (admin sessions only): accounts, the data nobody owns yet, and backups. An admin has no
 * data of their own, so this is the only page they see.
 */
export function AdminPage() {
  usePageTitle('Admin');
  const me = useMe();
  const timezone = me.data?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone;

  return (
    <>
      <TopBar variant="large" title="Admin" subtitle="Users, unassigned data and backups" />
      <div className="page">
        <div className="grid-panels" style={{ alignItems: 'start' }}>
          <UsersCard />
          {/* One column for the two smaller cards, so Backups sits beside Users on a wide screen. */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 16, minWidth: 0 }}>
            <UnassignedCard timezone={timezone} />
            <BackupsCard timezone={timezone} />
          </div>
        </div>
      </div>
    </>
  );
}
