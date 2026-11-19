import { SampleDataChip, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { ServerCard, UploadersCard, WatchCard } from './system/DevicesView';
import { useDevices, useSettings, useStorage } from './today/queries';

// Stage 3m: GET /api/devices and GET /api/storage. In the Android app this screen stays native;
// this is the browser version, and it shows only what the server knows (the phone's own settings,
// such as what it sends to the watch, live in the app).

export function DevicesPage() {
  usePageTitle('Devices and sync');
  const devices = useDevices(true);
  const storage = useStorage(true);
  const settings = useSettings();
  const timezone = settings.data?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone;
  const sample = devices.data?.sample ?? storage.data?.sample;
  return (
    <>
      <TopBar variant="back" backTo="/" title="Devices and sync" subtitle="Watch, phone and your server" aside={<SampleDataChip sample={sample} />} />
      <div className="page" aria-busy={devices.isPending || storage.isPending}>
        <div className="grid-panels" style={{ alignItems: 'start' }}>
          <WatchCard query={devices} timezone={timezone} />
          <ServerCard storage={storage} timezone={timezone} />
          <UploadersCard query={devices} timezone={timezone} />
        </div>
      </div>
    </>
  );
}
