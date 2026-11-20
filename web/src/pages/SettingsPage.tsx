import { Button, Card, ErrorState, Section, SampleDataChip, Skeleton, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { AccountCard } from './system/AccountCard';
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
//
// Four sections, in the order a person needs them: Account (who, password, two-factor), Timezone
// and goals, Phones, Garmin. Each section is a <Section> with its cards as children.

export function SettingsPage() {
  usePageTitle('Settings');
  const settings = useSettings();
  const timezone = settings.data?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone;

  return (
    <>
      <TopBar variant="back" backTo="/" title="Settings" subtitle="Account, goals, phones and Garmin" aside={<SampleDataChip sample={settings.data?.sample} />} />
      <div className="page">
        <div className="sections">
          <Section title="Account" hint="Who you are signed in as, and how you log in.">
            <AccountCard />
            <PasswordCard />
            <TwoFactorCard />
          </Section>

          <Section title="Timezone and goals" hint="Your days follow this timezone. Goals fill the progress bars.">
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
          </Section>

          <Section title="Phones" hint="The phones and tools that send files to this server.">
            <TokensCard timezone={timezone} />
          </Section>

          <Section title="Garmin" hint="Optional: pass your activities on to Garmin Connect too.">
            <GarminCard timezone={timezone} />
          </Section>
        </div>
      </div>
    </>
  );
}
