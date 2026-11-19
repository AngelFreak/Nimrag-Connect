import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { api, clearOfflineApiCache } from '../api/client';
import { Button, Card, CardHeader, ErrorState, SampleDataChip, Skeleton, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { GarminCard } from './system/GarminCard';
import { GoalsCard } from './system/GoalsCard';
import { TokensCard } from './system/TokensCard';
import { errorMessage } from './system/errors';
import { useSettings } from './today/queries';

// Stage 3m: GET/PATCH /api/settings (timezone, goals), GET/POST/DELETE /api/tokens, the Garmin
// Connect (Forward) section on /api/garmin, and Log out.

export function SettingsPage() {
  usePageTitle('Settings');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const settings = useSettings();
  const [busy, setBusy] = useState(false);
  const timezone = settings.data?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone;

  async function logout() {
    setBusy(true);
    try {
      await api.post('/api/logout', undefined, { redirectOn401: false });
    } catch {
      // Already logged out or server unreachable: leave the page either way.
    }
    await clearOfflineApiCache();
    queryClient.clear();
    void navigate('/login', { replace: true });
  }

  return (
    <>
      <TopBar
        variant="back"
        backTo="/"
        title="Settings"
        subtitle="Timezone, goals, device tokens and Garmin Connect"
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
          <Card padding="lg" aria-labelledby="session-title">
            <CardHeader title="Session" id="session-title" />
            <Button size="lg" onClick={() => void logout()} disabled={busy}>
              Log out
            </Button>
          </Card>
        </div>
      </div>
    </>
  );
}
