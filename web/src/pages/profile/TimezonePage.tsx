import { useState } from 'react';
import type { SettingsResponse } from '../../api/generated';
import { Button, Card, ErrorState, SelectField, Skeleton } from '../../components';
import { suggestedZone, zoneOptions } from '../../lib/timezone';
import { useSettings } from '../today/queries';
import { errorMessage } from '../system/errors';
import { useSaveSettings } from '../system/queries';
import { SubPage } from './common';

/** A pick is saved at once; there is no Save button. */
function ZoneForm({ settings }: { settings: SettingsResponse }) {
  const save = useSaveSettings();
  const [choice, setChoice] = useState(settings.timezone);
  const [saved, setSaved] = useState(false);
  const here = suggestedZone(settings.timezone);

  function apply(zone: string) {
    if (zone === settings.timezone) return;
    setChoice(zone);
    setSaved(false);
    save.mutate(
      { timezone: zone },
      {
        onSuccess: (s) => {
          setChoice(s.timezone);
          setSaved(true);
        },
        // Show the zone that is really saved again.
        onError: () => setChoice(settings.timezone),
      },
    );
  }

  return (
    <Card padding="lg">
      <div className="form">
        <SelectField label="Timezone" name="timezone" value={choice} disabled={save.isPending} onChange={(e) => apply(e.target.value)}>
          {zoneOptions(settings.timezone, choice).map((z) => (
            <option key={z} value={z}>
              {z}
            </option>
          ))}
        </SelectField>
        {here ? (
          <Button size="lg" disabled={save.isPending} onClick={() => apply(here)}>
            Use {here}
          </Button>
        ) : null}
        {saved ? (
          <p className="status-msg status-msg--ok" role="status">
            Saved.
          </p>
        ) : null}
        {save.isError ? (
          <p className="status-msg status-msg--error" role="alert">
            {errorMessage(save.error)}
          </p>
        ) : null}
      </div>
    </Card>
  );
}

/** Profile, Timezone: GET and PATCH /api/settings. Days are local days in this zone. */
export function TimezonePage() {
  const settings = useSettings();
  return (
    <SubPage title="Timezone" backTo="/profile?section=preferences">
      <p className="subpage__lede">Your days start and end in this timezone. It saves as soon as you pick one.</p>
      {settings.isPending ? (
        <Card padding="lg" aria-label="Loading the timezone">
          <Skeleton height={48} />
        </Card>
      ) : null}
      {settings.isError && !settings.data ? (
        <Card padding="lg">
          <ErrorState title="Could not load the settings" action={<Button onClick={() => void settings.refetch()}>Try again</Button>}>
            {errorMessage(settings.error)}
          </ErrorState>
        </Card>
      ) : null}
      {settings.data ? <ZoneForm settings={settings.data} /> : null}
    </SubPage>
  );
}
