import { BarSeries, Card, CardHeader, DataList, ProgressBar, SampleDataChip, StageBar, Legend, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';
import { STEPS_WEEK } from './sample';

// STUB (stage 3i wires it to GET /api/movement/{date}): static canvas values.

export function MovementPage() {
  usePageTitle('Movement');
  return (
    <>
      <TopBar variant="back" backTo="/" title="Movement" subtitle="Thu 19 Nov · week 47" aside={<SampleDataChip sample />} />
      <div className="page">
        <Card padding="lg">
          <CardHeader title="Steps today" aside="Goal 8,000" />
          <span className="stat__value" style={{ fontSize: 34 }}>
            6,240
          </span>
          <ProgressBar value={6240} max={8000} height={8} label="6,240 of 8,000 steps, 78 percent" />
          <span className="stat__sub">4.6 km · 7 floors climbed</span>
        </Card>
        <Card padding="lg">
          <CardHeader title="Steps this week" aside="Avg 7,800 a day" />
          <BarSeries
            values={[...STEPS_WEEK]}
            labels={['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']}
            goal={8000}
            max={10000}
            color="var(--steps)"
            belowGoalColor="var(--steps-dim)"
            height={100}
            label="Steps per day this week: Monday 9,120, Tuesday 7,480, Wednesday 8,360, Thursday 6,240 so far; the goal line is 8,000"
          />
        </Card>
        <Card padding="lg">
          <CardHeader title="Intensity minutes" aside="Weekly goal 150" />
          <span className="stat__value" style={{ fontSize: 28 }}>
            96 min
          </span>
          <StageBar
            total={150}
            label="96 of 150 intensity minutes: 52 moderate and 44 from vigorous activity"
            segments={[
              { key: 'mod', label: 'Moderate', value: 52, color: 'var(--zone-1)' },
              { key: 'vig', label: 'Vigorous', value: 44, color: 'var(--warn)' },
            ]}
          />
          <Legend
            items={[
              { key: 'mod', label: 'Moderate 52', color: 'var(--zone-1)' },
              { key: 'vig', label: 'Vigorous 22, counted double', color: 'var(--warn)' },
            ]}
          />
        </Card>
        <Card padding="lg">
          <CardHeader title="Calories today" />
          <DataList
            items={[
              { label: 'Active', value: '412 kcal' },
              { label: 'Resting', value: '1,780 kcal' },
              { label: 'Total', value: '2,192 kcal', strong: true },
            ]}
          />
        </Card>
      </div>
    </>
  );
}
