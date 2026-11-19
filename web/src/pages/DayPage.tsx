import { useParams } from 'react-router-dom';
import { Card, CardHeader, Legend, SeriesChart, StressBars, TopBar, type StressBucket } from '../components';
import { addDays, browserToday, fmtShortDay, isIsoDay } from '../lib/dates';
import { usePageTitle } from '../lib/hooks';
import { ENERGY_DAY, HR_24H, range } from './sample';

// STUB (stage 3g wires it to GET /api/days/{date}): static canvas values for any date.
// `/day/:date?` without a date means today.

const STRESS: StressBucket[] = [
  { level: 'rest', value: null, span: 6 },
  { level: 'low', value: 28 },
  { level: 'low', value: 36 },
  { level: 'low', value: 24 },
  { level: 'low', value: 32 },
  { level: 'low', value: 42 },
  { level: 'low', value: 30 },
  { level: 'medium', value: 56 },
  { level: 'medium', value: 64 },
  { level: 'medium', value: 50 },
  { level: 'low', value: 30 },
  { level: 'activity', value: null, span: 2 },
  { level: 'low', value: 34 },
  { level: 'low', value: 22 },
  { level: 'rest', value: null, span: 2 },
];

export function DayPage() {
  const { date: param } = useParams();
  const today = browserToday();
  const date = isIsoDay(param) ? param : today;
  const title = fmtShortDay(date);
  usePageTitle(title);
  return (
    <>
      <TopBar
        variant="pager"
        title={title}
        subtitle="Day view · sample data"
        prevLabel="Previous day"
        nextLabel="Next day"
        prev={{ to: `/day/${addDays(date, -1)}` }}
        next={date >= today ? null : { to: `/day/${addDays(date, 1)}` }}
      />
      <div className="page">
        <Card>
          <CardHeader title="Heart rate" aside="Resting 51 · High 148" />
          <SeriesChart
            label="Heart rate across the day"
            summary="Heart rate across the day: lowest overnight, a peak of 148 bpm during the evening strength session."
            x={range(HR_24H.length)}
            series={[{ label: 'Heart rate', color: 'heart', data: HR_24H }]}
            height={110}
          />
          <div className="chart__axis" aria-hidden="true">
            <span>00</span>
            <span>06</span>
            <span>12</span>
            <span>18</span>
            <span>24</span>
          </div>
        </Card>
        <Card>
          <CardHeader title="Stress" aside="Average 28 · mostly low" />
          <StressBars buckets={STRESS} label="Stress level across the day: resting overnight, low through the morning, a medium stretch mid-afternoon" />
          <Legend
            items={[
              { key: 'rest', label: 'Rest 9h 10m', color: 'var(--stress-rest)' },
              { key: 'low', label: 'Low 10h 30m', color: 'var(--stress-low)' },
              { key: 'medium', label: 'Medium 2h 10m', color: 'var(--stress-medium)' },
              { key: 'activity', label: 'Activity', color: 'var(--stress-activity)', outlined: true },
            ]}
          />
        </Card>
        <Card>
          <CardHeader title="Energy" aside="High 94 · Low 38" />
          <SeriesChart
            label="Energy across the day"
            summary="Energy across the day: charges overnight to 94, drains through the day to 38 after the evening workout."
            x={range(ENERGY_DAY.length)}
            series={[{ label: 'Energy', color: 'energy', data: ENERGY_DAY, area: true }]}
            yRange={[0, 100]}
            height={100}
          />
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13 }}>
            <span className="tone-energy">+52 charged</span>
            <span className="tone-warn">-56 drained</span>
          </div>
        </Card>
      </div>
    </>
  );
}
