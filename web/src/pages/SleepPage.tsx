import { useParams, useSearchParams } from 'react-router-dom';
import { isIsoDay } from '../lib/dates';
import { SleepRangeView } from './health/SleepRangeView';
import { LatestNight, SleepView } from './health/SleepView';

// Stage 3f: one night (GET /api/sleep/{date}) and the last 7/14/30 nights (GET /api/sleep?nights=).
// `/sleep/:date?` without a date goes to the latest night. `/sleep?range=7d|4w|1y` is the score and
// time asleep over a longer period (GET /api/sleep/history); the one-night view is the "1d" option.

export function SleepPage() {
  const { date } = useParams();
  const [params] = useSearchParams();
  const range = params.get('range');
  if (!date && (range === '7d' || range === '4w' || range === '1y')) return <SleepRangeView />;
  return isIsoDay(date) ? <SleepView key={date} date={date} /> : <LatestNight />;
}
