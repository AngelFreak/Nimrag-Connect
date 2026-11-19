import { Button, Card, SampleDataChip, SeriesChart, StatTile, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { WEIGHT_TREND, range } from './sample';

// STUB (stage 3k wires it to GET/POST/DELETE /api/body): static sample values.
// Body fat and waist are left unavailable to show the rule: no entry means "unavailable".

const ENTRIES = [
  { when: 'Tue 17 Nov · 07:10', what: 'Weight' },
  { when: 'Tue 10 Nov · 07:05', what: 'Weight' },
  { when: 'Tue 3 Nov · 07:20', what: 'Weight, waist' },
  { when: 'Tue 27 Oct · 07:15', what: 'Weight' },
];

export function BodyPage() {
  usePageTitle('Body');
  return (
    <>
      <TopBar variant="back" backTo="/health" title="Body" subtitle="Weight and measurements" aside={<SampleDataChip sample />} />
      <div className="page">
        <Card padding="lg">
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', gap: 12 }}>
            <div className="stat">
              <h2 className="card__title card__title--muted">Weight · Tue 17 Nov</h2>
              <span className="stat__value" style={{ fontSize: 34 }}>
                80.8 kg
              </span>
            </div>
            <span className="stat__sub">30-day change: -1.4 kg</span>
          </div>
          <SeriesChart
            label="Weight trend over 90 days"
            summary="Weight trend over 90 days: a smoothed line falling from 82.4 kg to 80.8 kg."
            x={range(WEIGHT_TREND.length)}
            series={[{ label: 'Weight', color: 'link', data: WEIGHT_TREND }]}
            height={120}
          />
          <div className="chart__axis" aria-hidden="true">
            <span>21 Aug</span>
            <span>5 Oct</span>
            <span>17 Nov</span>
          </div>
        </Card>
        <div className="grid-2">
          <StatTile label="Body fat" value={null} sub="Manual or scale entry" />
          <StatTile label="Waist" value={null} sub="Manual entry" />
        </div>
        <Card padding="list">
          <h2 className="card__title card__title--muted" style={{ padding: '10px 0 6px' }}>
            Entries
          </h2>
          <ul>
            {ENTRIES.map((e) => (
              <li className="plain-row" key={e.when}>
                <span>{e.when}</span>
                <span className="plain-row__sub">{e.what}</span>
              </li>
            ))}
          </ul>
        </Card>
      </div>
      <div className="footer-bar footer-bar--single">
        <Button size="lg" variant="primary" block>
          Add entry
        </Button>
      </div>
    </>
  );
}
