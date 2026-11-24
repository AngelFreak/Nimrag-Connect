import { Link } from 'react-router-dom';
import { Ring, Skeleton } from '../../components';
import { LEVEL_COLOR, LEVEL_NAME } from './readiness';
import { useReadiness } from './readinessQueries';
import './readiness.css';

/** The card at the top of the Training page: today's readiness estimate, linking to its page. */
export function ReadinessCard({ flat }: { flat?: boolean }) {
  const q = useReadiness();
  const data = q.data;
  const score = data?.score ?? null;
  const level = data?.level ?? null;
  const color = level ? LEVEL_COLOR[level] : 'var(--accent)';
  let sub: string;
  if (q.isPending) sub = '';
  else if (!data) sub = 'Could not be loaded. Open the page to try again.';
  else if (score === null) sub = data.unavailable ?? 'Not enough data yet.';
  else sub = data.headline ?? '';
  return (
    <Link to="/training/readiness" className={flat ? 'ready-card ready-card--flat' : 'card card--lg ready-card'} aria-busy={q.isPending}>
      {q.isPending ? <Skeleton height={72} width="72px" /> : <Ring value={score} max={100} label="Readiness estimate" color={color} size={72} strokeWidth={8} />}
      <span className="ready-card__text">
        <span className="ready-card__title">Readiness, Nimrag estimate</span>
        {q.isPending ? (
          <Skeleton height={24} width="50%" />
        ) : (
          <span className="ready-card__level">
            {level ? <span className="rate-dot" aria-hidden="true" style={{ background: color }} /> : null}
            {level ? LEVEL_NAME[level] : 'No estimate yet'}
          </span>
        )}
        <span className="ready-card__sub">{sub}</span>
      </span>
      <span className="ready-card__chevron" aria-hidden="true">
        ›
      </span>
    </Link>
  );
}
