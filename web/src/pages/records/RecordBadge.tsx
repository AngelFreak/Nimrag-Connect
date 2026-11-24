import { Link } from 'react-router-dom';
import { badgeText } from './format';
import './records.css';

/** A small cup mark; decorative, the text next to it says it all. */
export function CupIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
      <path d="M7 4h10v5a5 5 0 0 1-10 0V4zM7 6H4v1.5A3.5 3.5 0 0 0 7.5 11M17 6h3v1.5a3.5 3.5 0 0 1-3.5 3.5M12 14v4M8.5 20h7" />
    </svg>
  );
}

/** "Personal record: 5 km, 10 km", a link to the records page; nothing when the activity holds none. */
export function RecordBadge({ kinds }: { kinds: readonly string[] | undefined }) {
  const text = badgeText(kinds ?? []);
  if (text === null) return null;
  return (
    <Link to="/records" className="rec-badge">
      <span className="rec-badge__icon">
        <CupIcon />
      </span>
      {text}
    </Link>
  );
}
