import { useParams } from 'react-router-dom';
import { isIsoDay } from '../lib/dates';
import { LatestNight, SleepView } from './health/SleepView';

// Stage 3f: one night (GET /api/sleep/{date}) and the last 7/14/30 nights (GET /api/sleep?nights=).
// `/sleep/:date?` without a date goes to the latest night.

export function SleepPage() {
  const { date } = useParams();
  return isIsoDay(date) ? <SleepView key={date} date={date} /> : <LatestNight />;
}
