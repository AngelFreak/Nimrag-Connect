import { Link } from 'react-router-dom';
import { useState } from 'react';
import { BarSeries, Card, CardHeader, DataList, Ring, SampleDataChip, SegmentedControl, SeriesChart, StatTile, Table, TopBar, ProgressBar, Button } from '../components';
import { useIsDesktop, usePageTitle } from '../lib/hooks';
import { HRV_30, HR_24H, SLEEP_14, range } from './sample';

// STUB (stage 3c wires it to GET /api/today): static canvas values.

type RangeKey = 'day' | 'week' | 'month' | 'year';
const RANGES = [
  { value: 'day', label: 'Day' },
  { value: 'week', label: 'Week' },
  { value: 'month', label: 'Month' },
  { value: 'year', label: 'Year' },
] as const;

const RECENT = [
  { id: '1', date: 'Wed 18 Nov', type: 'Strength', duration: '42 min', detail: '18 sets · 3,300 kg', hr: '112' },
  { id: '2', date: 'Mon 16 Nov', type: 'Run', duration: '38 min', detail: '6.4 km · 5:56 /km', hr: '151' },
  { id: '3', date: 'Sun 15 Nov', type: 'Strength', duration: '45 min', detail: '18 sets · 3,180 kg', hr: '109' },
  { id: '4', date: 'Fri 13 Nov', type: 'Walk', duration: '52 min', detail: '4.8 km', hr: '98' },
];

function PhoneToday() {
  return (
    <>
      <TopBar
        variant="large"
        subtitle="Thursday 19 November"
        title="Today"
        aside={
          <>
            <SampleDataChip sample />
            <span>Watch synced 07:42</span>
          </>
        }
      />
      <div className="page">
        <Card padding="lg" row aria-label="Energy">
          <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
            <Ring value={68} label="Energy level" color="var(--energy)" />
            <div className="stat" style={{ gap: 6 }}>
              <span className="stat__label">Energy</span>
              <span style={{ fontSize: 17, fontWeight: 600 }}>Good reserve for training</span>
              <div style={{ display: 'flex', gap: 14, fontSize: 13 }}>
                <span className="tone-energy">+54 charged</span>
                <span className="tone-warn">-31 drained</span>
              </div>
            </div>
          </div>
        </Card>
        <div className="grid-2">
          <StatTile label="Sleep" value="7h 12m" sub="Score 81" tone="sleep" />
          <StatTile label="Overnight HRV" value="62 ms" sub="Within your range" tone="link" />
          <StatTile label="Resting heart rate" value="51 bpm" sub="7-day average 52" />
          <StatTile label="Steps" value="6,240" sub="Goal 8,000">
            <ProgressBar value={6240} max={8000} label="6,240 of 8,000 steps" />
          </StatTile>
        </div>
        <Card>
          <CardHeader title="Heart rate, last 24 hours" tone="muted" aside="48 to 148 bpm" />
          <SeriesChart
            label="Heart rate over the last 24 hours"
            summary="Heart rate over the last 24 hours: low overnight at 48 bpm, with a peak of 148 bpm during an evening workout."
            x={range(HR_24H.length)}
            series={[{ label: 'Heart rate', color: 'heart', data: HR_24H }]}
            height={64}
          />
        </Card>
        <Card row>
          <div className="stat">
            <span className="stat__label">Last activity · yesterday</span>
            <span style={{ fontSize: 16, fontWeight: 600 }}>Strength, 42 min</span>
            <span className="stat__sub">6 exercises · 18 sets</span>
          </div>
          <Link to="/activities/1" style={{ fontWeight: 500, minHeight: 44, display: 'flex', alignItems: 'center', padding: '0 4px' }}>
            Open
          </Link>
        </Card>
      </div>
    </>
  );
}

function DesktopOverview() {
  const [rng, setRng] = useState<RangeKey>('month');
  return (
    <>
      <TopBar
        variant="large"
        subtitle="Thursday 19 November 2026"
        title="Overview"
        aside={
          <div className="topbar__aside--row" style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
            <SampleDataChip sample />
            <SegmentedControl label="Time range" options={RANGES} value={rng} onChange={setRng} />
          </div>
        }
      />
      <div className="page">
        <div className="grid-tiles">
          <StatTile size="xl" label="Energy now" value="68" sub="+54 charged overnight" tone="energy" />
          <StatTile size="xl" label="Sleep last night" value="7h 12m" sub="Score 81" tone="sleep" />
          <StatTile size="xl" label="Overnight HRV" value="62 ms" sub="Range 55 to 70" />
          <StatTile size="xl" label="Resting heart rate" value="51 bpm" sub="7-day average 52" />
          <StatTile size="xl" label="Training this week" value="3 sessions" sub="2h 31m total" />
        </div>
        <div className="grid-panels">
          <Card padding="xl">
            <CardHeader title="HRV, last 30 nights" aside="Shaded band is your usual range" tone="lg" />
            <SeriesChart
              label="Overnight HRV for the last 30 nights"
              summary="Overnight HRV for the last 30 nights, mostly inside the usual range of 55 to 70 milliseconds with a dip nine days ago."
              x={range(HRV_30.length)}
              series={[{ label: 'HRV', color: 'link', data: HRV_30 }]}
              band={[55, 70]}
              height={200}
              axes="y"
            />
            <div className="chart__axis" aria-hidden="true">
              <span>20 Oct</span>
              <span>27 Oct</span>
              <span>3 Nov</span>
              <span>10 Nov</span>
              <span>18 Nov</span>
            </div>
          </Card>
          <Card padding="xl">
            <CardHeader title="Sleep, last 14 nights" aside="Average 6h 58m" tone="lg" />
            <BarSeries
              values={SLEEP_14}
              labels={range(SLEEP_14.length).map(String)}
              axisLabels={['5 Nov', '9 Nov', '13 Nov', '18 Nov']}
              label="Sleep duration for the last 14 nights, between 5 hours 30 minutes and 7 hours 40 minutes"
              highlightLast="var(--sleep)"
              height={200}
            />
          </Card>
        </div>
        <div className="grid-panels">
          <Card padding="xl">
            <CardHeader title="Recent activities" tone="lg" aside={<Link to="/activities">View all</Link>} />
            <Table
              caption="Recent activities"
              wide
              rows={RECENT}
              rowKey={(r) => r.id}
              columns={[
                { key: 'date', header: 'Date', cell: (r) => r.date },
                { key: 'type', header: 'Type', cell: (r) => r.type },
                { key: 'dur', header: 'Duration', cell: (r) => r.duration },
                { key: 'detail', header: 'Detail', cell: (r) => r.detail, muted: true },
                { key: 'hr', header: 'Avg HR', cell: (r) => r.hr },
              ]}
            />
          </Card>
          <Card padding="xl">
            <CardHeader title="Sync and storage" tone="lg" />
            <p style={{ color: 'var(--text-2)', fontSize: 14 }}>Everything comes from the watch over Bluetooth and stays on your server. Original files are kept so data can be re-read later.</p>
            <DataList
              layout="wide"
              items={[
                { label: 'Server', value: window.location.host },
                { label: 'Device', value: 'Instinct 2 · battery 74%' },
                { label: 'Last sync', value: 'Today 07:42 · 5 new files' },
                { label: 'Files stored', value: '1,284 original files · 212 MB' },
                { label: 'Waiting on phone', value: '0 files' },
                { label: 'Last backup', value: 'Today 03:00' },
              ]}
            />
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 12, marginTop: 4 }}>
              <Button variant="primary">Export all data</Button>
              <Button>Re-read original files</Button>
            </div>
          </Card>
        </div>
      </div>
    </>
  );
}

export function TodayPage() {
  const desktop = useIsDesktop();
  usePageTitle(desktop ? 'Overview' : 'Today');
  return desktop ? <DesktopOverview /> : <PhoneToday />;
}
