import { useParams } from 'react-router-dom';
import { Card, CardHeader, MapView, SampleDataChip, StatTile, Table, TopBar, ZoneBars } from '../components';
import { usePageTitle } from '../lib/hooks';

// STUB (stage 3d wires it to GET /api/activities/{id} and /track): static canvas values.

const LAPS = [
  { n: 1, dist: '1.00 km', pace: '6:05', hr: '138' },
  { n: 2, dist: '1.00 km', pace: '5:58', hr: '147' },
  { n: 3, dist: '1.00 km', pace: '5:52', hr: '152' },
  { n: 4, dist: '1.00 km', pace: '5:55', hr: '154' },
  { n: 5, dist: '1.00 km', pace: '5:49', hr: '157' },
  { n: 6, dist: '1.00 km', pace: '5:57', hr: '156' },
  { n: 7, dist: '0.40 km', pace: '6:00', hr: '155' },
];

export function ActivityDetailPage() {
  const { id } = useParams();
  usePageTitle('Run');
  return (
    <>
      <TopBar variant="back" backTo="/activities" title="Run" subtitle="Mon 16 Nov · 17:40" aside={<SampleDataChip sample />} />
      <div className="page" data-activity-id={id}>
        <Card padding="flush">
          <MapView label="Route map" />
        </Card>
        <div className="grid-3">
          <StatTile size="sm" label="Distance" value="6.4 km" />
          <StatTile size="sm" label="Time" value="38:00" />
          <StatTile size="sm" label="Pace" value="5:56 /km" />
          <StatTile size="sm" label="Avg HR" value="151 bpm" />
          <StatTile size="sm" label="Climb" value="42 m" />
          <StatTile size="sm" label="Energy" value="468 kcal" />
        </div>
        <Card>
          <CardHeader title="Time in heart rate zones" tone="muted" />
          <ZoneBars
            zones={[
              { zone: 5, minutes: 1 },
              { zone: 4, minutes: 8 },
              { zone: 3, minutes: 18 },
              { zone: 2, minutes: 9 },
              { zone: 1, minutes: 2 },
            ]}
          />
        </Card>
        <Card>
          <CardHeader title="Laps" tone="muted" />
          <Table
            caption="Laps"
            density="compact"
            rows={LAPS}
            rowKey={(l) => String(l.n)}
            columns={[
              { key: 'n', header: 'Lap', cell: (l) => l.n },
              { key: 'd', header: 'Distance', cell: (l) => l.dist },
              { key: 'p', header: 'Pace', cell: (l) => l.pace },
              { key: 'hr', header: 'HR', cell: (l) => l.hr, align: 'end' },
            ]}
          />
        </Card>
      </div>
    </>
  );
}
