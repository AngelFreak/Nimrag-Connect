import { Card, CardHeader, Button, CheckRow, DataList, SampleDataChip, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { Link } from 'react-router-dom';

// STUB (stage 3m wires it to GET /api/devices and /api/storage): static canvas values.
// In the Android app this screen stays native; this is the browser version.

export function DevicesPage() {
  usePageTitle('Devices and sync');
  return (
    <>
      <TopBar variant="back" backTo="/" title="Devices and sync" subtitle="Watch, phone and your server" aside={<SampleDataChip sample />} />
      <div className="page">
        <Card padding="lg" aria-label="Watch">
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <div className="stat" style={{ gap: 3 }}>
              <h2 style={{ fontSize: 17, fontWeight: 600 }}>Instinct 2</h2>
              <span className="stat__sub tone-link">Connected over Bluetooth</span>
            </div>
            <span className="stat__value" style={{ fontSize: 22 }}>
              74%
            </span>
          </div>
          <DataList
            items={[
              { label: 'Last sync', value: 'Today 07:42' },
              { label: 'Files pulled', value: '5 new' },
              { label: 'Firmware', value: null },
            ]}
          />
          <Button size="lg" variant="primary">
            Sync now
          </Button>
        </Card>
        <Card padding="lg">
          <CardHeader title="Your server" />
          <DataList
            items={[
              { label: 'Address', value: window.location.host },
              { label: 'Status', value: 'Reachable', tone: 'link' },
              { label: 'Waiting to upload', value: '0 files' },
              { label: 'Upload over', value: 'Wi-Fi and VPN only' },
            ]}
          />
        </Card>
        <Card padding="list">
          <h2 className="card__title" style={{ padding: '10px 0 4px' }}>
            Phone sends to watch
          </h2>
          <CheckRow label="Time and time zone" defaultChecked />
          <CheckRow label="GPS assist data" defaultChecked />
          <CheckRow label="Weather" />
          <CheckRow label="Notifications" defaultChecked />
        </Card>
        <Card padding="list">
          <h2 className="card__title" style={{ padding: '10px 0 4px' }}>
            Data
          </h2>
          <Link to="/devices" className="plain-row" style={{ minHeight: 48 }}>
            Export all data<span className="plain-row__sub">Original files and CSV</span>
          </Link>
          <Link to="/devices" className="plain-row" style={{ minHeight: 48 }}>
            Import history<span className="plain-row__sub">From an export archive</span>
          </Link>
          <Link to="/settings" className="plain-row" style={{ minHeight: 48 }}>
            Units<span className="plain-row__sub">Metric</span>
          </Link>
        </Card>
      </div>
    </>
  );
}
