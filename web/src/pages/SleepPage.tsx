import { useParams } from 'react-router-dom';
import { BarSeries, Card, CardHeader, Legend, SampleDataChip, StageTimeline, StatTile, TopBar, type StageSpan } from '../components';
import { usePageTitle } from '../lib/hooks';
import { SLEEP_7 } from './sample';

// STUB (stage 3f wires it to GET /api/sleep/{date} and /api/sleep?nights=7): static canvas values.
// `/sleep/:date?` without a date means the latest night.

// Minutes from 23:18 (444 min night).
const SPANS: StageSpan[] = [
  { stage: 'light', from: 0, to: 25 },
  { stage: 'deep', from: 25, to: 85 },
  { stage: 'light', from: 85, to: 127 },
  { stage: 'rem', from: 127, to: 158 },
  { stage: 'light', from: 158, to: 205 },
  { stage: 'deep', from: 205, to: 230 },
  { stage: 'awake', from: 230, to: 237 },
  { stage: 'light', from: 237, to: 290 },
  { stage: 'rem', from: 290, to: 329 },
  { stage: 'light', from: 329, to: 379 },
  { stage: 'awake', from: 379, to: 385 },
  { stage: 'rem', from: 385, to: 430 },
  { stage: 'light', from: 430, to: 444 },
];

export function SleepPage() {
  const { date } = useParams();
  usePageTitle('Sleep');
  return (
    <>
      <TopBar variant="back" backTo="/" title="Sleep" subtitle="Night of Wed 18 Nov" aside={<SampleDataChip sample />} />
      <div className="page" data-date={date}>
        <Card padding="lg" row>
          <div className="stat stat--lg">
            <span className="stat__label">Time asleep</span>
            <span className="stat__value">7h 12m</span>
            <span className="stat__sub">23:18 to 06:42</span>
          </div>
          <div className="stat stat--lg" style={{ alignItems: 'flex-end' }}>
            <span className="stat__label">Score</span>
            <span className="stat__value tone-sleep">81</span>
            <span className="stat__sub tone-sleep">Good</span>
          </div>
        </Card>
        <Card padding="lg">
          <CardHeader title="Stages through the night" tone="muted" />
          <StageTimeline
            spans={SPANS}
            totalMinutes={444}
            label="Sleep stages: deep sleep early in the night, REM periods growing longer toward morning, two short awakenings"
            ticks={['23:18', '01:00', '03:00', '05:00', '06:42']}
          />
          <Legend
            grid
            items={[
              { key: 'deep', label: 'Deep', color: 'var(--stage-deep)', value: '1h 24m' },
              { key: 'light', label: 'Light', color: 'var(--stage-light)', value: '3h 58m' },
              { key: 'rem', label: 'REM', color: 'var(--stage-rem)', value: '1h 50m' },
              { key: 'awake', label: 'Awake', color: 'var(--stage-awake)', value: '12m' },
            ]}
          />
        </Card>
        <div className="grid-3">
          <StatTile size="sm" label="HRV" value="62 ms" />
          <StatTile size="sm" label="Lowest HR" value="48 bpm" />
          <StatTile size="sm" label="Breathing" value="13.4 /min" />
        </div>
        <Card padding="lg">
          <CardHeader title="Last 7 nights" tone="muted" aside="Average 6h 58m" />
          <BarSeries
            values={SLEEP_7}
            labels={['Thu', 'Fri', 'Sat', 'Sun', 'Mon', 'Tue', 'Wed']}
            label="Sleep duration for the last 7 nights, between 6 hours 5 minutes and 7 hours 30 minutes"
            highlightLast="var(--sleep)"
          />
        </Card>
      </div>
    </>
  );
}
