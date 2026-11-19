import { useParams } from 'react-router-dom';
import { Button, Card, CardHeader, SampleDataChip, SeriesChart, StatTile, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { range } from './sample';

// STUB (stage 3e wires it to GET/PATCH /api/activities/{id}/strength): static canvas values.

const HR_SESSION = [30, 54, 38, 62, 44, 70, 48, 62, 40, 78, 52, 74, 46, 96, 58, 90, 50, 104, 60, 74, 42, 66, 36];

const EXERCISES = [
  { name: 'Dumbbell bench press', sets: '3 sets · 10, 10, 10 reps', load: '22 kg' },
  { name: 'Bent-over row', sets: '3 sets · 10, 10, 10 reps', load: '20 kg' },
  { name: 'Goblet squat', sets: '3 sets · 12, 12, 12 reps', load: '24 kg' },
  { name: 'Overhead press', sets: '3 sets · 8, 8, 8 reps', load: '14 kg' },
  { name: 'Romanian deadlift', sets: '3 sets · 10, 10, 10 reps', load: '28 kg' },
  { name: 'Plank', sets: '3 sets · 60 s each', load: 'Bodyweight' },
];

export function StrengthPage() {
  const { id } = useParams();
  usePageTitle('Strength session');
  return (
    <>
      <TopBar variant="back" backTo="/activities?type=strength" title="Strength" subtitle="Wed 18 Nov · 18:05" aside={<SampleDataChip sample />} />
      <div className="page" data-activity-id={id}>
        <div className="grid-3">
          <StatTile size="sm" label="Duration" value="42 min" />
          <StatTile size="sm" label="Sets" value="18" />
          <StatTile size="sm" label="Volume" value="3,300 kg" />
        </div>
        <Card>
          <CardHeader title="Heart rate" tone="muted" aside="Avg 112 · Max 148 bpm" />
          <SeriesChart
            label="Heart rate during the session"
            summary="Heart rate during the session, rising with each set and dropping in the rests. Average 112, maximum 148 bpm."
            x={range(HR_SESSION.length)}
            series={[{ label: 'Heart rate', color: 'heart', data: HR_SESSION }]}
            height={60}
          />
        </Card>
        <Card padding="list">
          <h2 className="sr-only">Exercises</h2>
          <ul>
            {EXERCISES.map((e) => (
              <li className="exercise" key={e.name}>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                  <span className="exercise__name">{e.name}</span>
                  <span className="exercise__sub">{e.sets}</span>
                </div>
                <span style={{ fontSize: 15, color: e.load === 'Bodyweight' ? 'var(--text-2)' : undefined }}>{e.load}</span>
              </li>
            ))}
          </ul>
        </Card>
      </div>
      <div className="footer-bar">
        <Button size="lg">Edit sets</Button>
        <Button size="lg" variant="primary">
          Compare to last
        </Button>
      </div>
    </>
  );
}
