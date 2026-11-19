import { useParams } from 'react-router-dom';
import { browserToday, isIsoDay } from '../lib/dates';
import { DayView } from './health/DayView';

// Stage 3g: one day (GET /api/days/{date}): heart rate, stress, energy, steps, the night, activities.
// `/day/:date?` without a date means today; previous and next come from the response.

export function DayPage() {
  const { date: param } = useParams();
  return <DayView date={isIsoDay(param) ? param : browserToday()} />;
}
