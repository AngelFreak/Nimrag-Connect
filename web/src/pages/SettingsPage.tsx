import { useLogout, useMe } from '../account/queries';
import { Button, Card, CardHeader, ErrorState, SampleDataChip, Skeleton, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { GarminCard } from './system/GarminCard';
import { GoalsCard } from './system/GoalsCard';
import { PasswordCard } from './system/PasswordCard';
import { TokensCard } from './system/TokensCard';
import { TwoFactorCard } from './system/TwoFactorCard';
import { errorMessage } from './system/errors';
import { useSettings } from './today/queries';

// Stage 3m: GET/PATCH /api/settings (timezone, goals), GET/POST/DELETE /api/tokens, the Garmin
// Connect (Forward) section on /api/garmin, and Log out. Accounts: Two-factor (/api/me/2fa) and
// Change password (/api/me/password).

export function SettingsPage() {
  usePageTitle('Settings');
  const settings = useSettings();
  const me = useMe();
  const logout = useLogout();
  const timezone = settings.data?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone;

  return (
    <>
      <TopBar
        variant="back"
        backTo="/"
        title="Settings"
        subtitle="Timezone, goals, device tokens, two-factor and password"
        aside={<SampleDataChip sample={settings.data?.sample} />}
      />
      <div className="page">
        <div className="grid-panels" style={{ alignItems: 'start' }}>
          {settings.isPending ? (
            <Card padding="lg" aria-label="Loading settings">
              <Skeleton height={20} width="40%" />
              <Skeleton height={48} lines={3} />
            </Card>
          ) : settings.data ? (
            <GoalsCard settings={settings.data} />
          ) : (
            <Card padding="lg">
              <ErrorState title="Could not load the settings" action={<Button onClick={() => void settings.refetch()}>Try again</Button>}>
                {errorMessage(settings.error)}
              </ErrorState>
            </Card>
          )}
          <TokensCard timezone={timezone} />
          <GarminCard timezone={timezone} />
          <TwoFactorCard />
          <PasswordCard />
          <Card padding="lg" aria-labelledby="session-title">
            <CardHeader title="Session" id="session-title" aside={me.data ? `Signed in as ${me.data.username}` : undefined} />
            <Button size="lg" onClick={() => logout.mutate()} disabled={logout.isPending}>
              Log out
            </Button>
          </Card>
        </div>
      </div>
    </>
  );
}
