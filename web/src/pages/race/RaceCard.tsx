import { Link } from 'react-router-dom';
import { Skeleton } from '../../components';
import { distanceName, fmtRaceTime } from './model';
import { useRacePredictions } from './queries';
import '../health/readiness.css';

/** The link to the race predictor on the Training page: the 5 km estimate and what it is. */
export function RaceCard({ flat }: { flat?: boolean }) {
  const q = useRacePredictions();
  const five = q.data?.predictions.find((p) => p.distance_m === 5000);
  let sub: string;
  if (q.isPending) sub = '';
  else if (!q.data) sub = 'Could not be loaded. Open the page to try again.';
  else if (!five) sub = 'A run with GPS in the last 6 weeks is needed.';
  else sub = `${distanceName(five.distance_m)} in ${fmtRaceTime(five.seconds)}`;
  return (
    <Link to="/training/race-predictor" className={flat ? 'ready-card ready-card--flat' : 'card card--lg ready-card'} aria-busy={q.isPending}>
      <span className="ready-card__text">
        <span className="ready-card__title">Race predictor, Nimrag estimate</span>
        {q.isPending ? <Skeleton height={20} width="50%" /> : <span className="ready-card__level">{sub}</span>}
        <span className="ready-card__sub">5 km, 10 km, half marathon and marathon</span>
      </span>
      <span className="ready-card__chevron" aria-hidden="true">
        ›
      </span>
    </Link>
  );
}
