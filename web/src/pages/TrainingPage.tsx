import { BarList, Card, CardHeader, RangeBar, Ring, SampleDataChip, SeriesChart, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { LOAD_28, VO2_90, range } from './sample';

// STUB (stage 3j wires it to GET /api/training): static canvas values.
// VO2 max here is derived (max MET x 3.5) and must be labelled as derived when wired.

export function TrainingPage() {
  usePageTitle('Training');
  return (
    <>
      <TopBar variant="back" backTo="/health" title="Training" subtitle="As of Thu 19 Nov" aside={<SampleDataChip sample />} />
      <div className="page">
        <Card padding="lg" row>
          <div className="stat" style={{ gap: 4 }}>
            <h2 className="card__title card__title--muted">Recovery time left</h2>
            <span className="stat__value" style={{ fontSize: 34 }}>
              14 h
            </span>
            <span className="stat__sub tone-link">Ready for a hard session Fri 09:00</span>
          </div>
          <Ring value={61} label="Recovery complete, percent" color="var(--accent)" size={72} strokeWidth={8} showValue={false} />
        </Card>
        <Card padding="lg">
          <CardHeader title="7-day load" aside="Target range 240 to 420" />
          <span className="stat__value" style={{ fontSize: 28 }}>
            312
          </span>
          <RangeBar value={312} min={0} max={600} target={[240, 420]} label="Load of 312 sits inside the target range of 240 to 420" />
          <SeriesChart
            label="Load over the last 4 weeks"
            summary="Load over the last 4 weeks, steady between 280 and 340."
            x={range(LOAD_28.length)}
            series={[{ label: 'Load', color: 'link', data: LOAD_28 }]}
            height={64}
          />
          <span className="stat__sub">Steady for 4 weeks</span>
        </Card>
        <Card padding="lg">
          <CardHeader title="Load by intensity, 4 weeks" />
          <BarList
            label="Load by intensity, 4 weeks"
            max={900}
            items={[
              { key: 'easy', label: 'Easy aerobic', value: 412, color: 'var(--stress-rest)' },
              { key: 'hard', label: 'Hard aerobic', value: 556, color: 'var(--accent)' },
              { key: 'an', label: 'Anaerobic', value: 128, color: 'var(--warn)' },
            ]}
          />
        </Card>
        <Card padding="lg">
          <CardHeader title="VO2 max (derived)" aside={<span className="tone-link">+1 in 3 months</span>} />
          <span className="stat__value" style={{ fontSize: 28 }}>
            46
          </span>
          <SeriesChart
            label="VO2 max over the last 3 months"
            summary="VO2 max over the last 3 months, rising from 45 to 46."
            x={range(VO2_90.length)}
            series={[{ label: 'VO2 max', color: 'linkSoft', data: VO2_90 }]}
            height={56}
          />
        </Card>
      </div>
    </>
  );
}
