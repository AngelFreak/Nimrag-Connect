import { useState } from 'react';
import { Card, CardHeader, SampleDataChip, SegmentedControl, SeriesChart, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { BREATHING_28, HRV_30, RHR_28, SLEEP_SCORE_28, SPO2_28, range } from './sample';

// STUB (stage 3h wires it to GET /api/trends?range=7d|4w|3m|1y): static canvas values; the
// selected range is state only and does not change the data yet.

const RANGES = [
  { value: '7d', label: '7 days' },
  { value: '4w', label: '4 weeks' },
  { value: '3m', label: '3 months' },
  { value: '1y', label: '1 year' },
] as const;
type RangeKey = (typeof RANGES)[number]['value'];

export function HealthPage() {
  usePageTitle('Health');
  const [rng, setRng] = useState<RangeKey>('4w');
  const when = RANGES.find((r) => r.value === rng)?.label ?? '';
  return (
    <>
      <TopBar variant="large" title="Health" aside={<SampleDataChip sample />} />
      <SegmentedControl label="Time range" options={RANGES} value={rng} onChange={setRng} layout="grid" />
      <div className="page" style={{ paddingTop: 0 }}>
        <Card>
          <CardHeader title="Overnight HRV" aside="Avg 61 ms · range 55 to 70" />
          <SeriesChart
            label={`Overnight HRV over ${when}`}
            summary={`Overnight HRV over ${when}, mostly inside the usual range of 55 to 70 milliseconds with one dip in the third week.`}
            x={range(HRV_30.length)}
            series={[{ label: 'HRV', color: 'link', data: HRV_30 }]}
            band={[55, 70]}
            height={90}
          />
        </Card>
        <Card>
          <CardHeader title="Resting heart rate" aside="Avg 52 bpm · 50 to 56" />
          <SeriesChart
            label={`Resting heart rate over ${when}`}
            summary={`Resting heart rate over ${when}, steady around 52 with a rise to 60 in the third week.`}
            x={range(RHR_28.length)}
            series={[{ label: 'Resting heart rate', color: 'heart', data: RHR_28 }]}
            height={80}
          />
        </Card>
        <div className="grid-2">
          <Card padding="tile" className="stat" aria-label="Breathing rate">
            <h2 className="card__title card__title--muted">Breathing rate</h2>
            <span className="stat__value" style={{ fontSize: 22 }}>
              13.4 /min
            </span>
            <span className="stat__sub">Asleep · awake 14.8</span>
            <SeriesChart label={`Breathing rate over ${when}`} summary={`Breathing rate over ${when}, flat around 13.4 breaths per minute.`} x={range(BREATHING_28.length)} series={[{ label: 'Breathing rate', color: 'link', data: BREATHING_28, width: 2 }]} height={36} />
          </Card>
          <Card padding="tile" className="stat" aria-label="Blood oxygen">
            <h2 className="card__title card__title--muted">Blood oxygen</h2>
            <span className="stat__value" style={{ fontSize: 22 }}>
              95%
            </span>
            <span className="stat__sub">Overnight avg · low 91%</span>
            <SeriesChart label={`Overnight blood oxygen over ${when}`} summary={`Overnight blood oxygen over ${when}, between 94 and 96 percent.`} x={range(SPO2_28.length)} series={[{ label: 'Blood oxygen', color: 'link', data: SPO2_28, width: 2 }]} height={36} />
          </Card>
        </div>
        <Card>
          <CardHeader title="Sleep score" aside="Avg 78" />
          <SeriesChart
            label={`Sleep score over ${when}`}
            summary={`Sleep score over ${when}, between 64 and 88.`}
            x={range(SLEEP_SCORE_28.length)}
            series={[{ label: 'Sleep score', color: 'sleep', data: SLEEP_SCORE_28 }]}
            height={70}
          />
        </Card>
      </div>
    </>
  );
}
