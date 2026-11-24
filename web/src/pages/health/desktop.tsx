import { useId, type ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { BackIcon, CalendarIcon, NextIcon } from '../../components';
import './health.css';

// Building blocks of the stats pages on a wide screen (docs/design/web-layout/SPEC.md, pattern A).
// A phone never renders these; each page picks between its phone layout and this one with `useIsDesktop()`.

/** Two columns: 320 px for the score and summary, the rest for factors and charts. */
export function StatsColumns({ side, children, asideRight }: { side: ReactNode; children: ReactNode; asideRight?: boolean }) {
  return (
    <div className={asideRight ? 'stats-cols stats-cols--aside-right' : 'stats-cols'}>
      {asideRight ? (
        <>
          <div className="stats-main">{children}</div>
          <div className="stats-side">{side}</div>
        </>
      ) : (
        <>
          <div className="stats-side">{side}</div>
          <div className="stats-main">{children}</div>
        </>
      )}
    </div>
  );
}

/** A section: a 22/300 title (with a small note at the right) and its content. */
export function StatsSection({ title, aside, children, className }: { title: ReactNode; aside?: ReactNode; children: ReactNode; className?: string }) {
  const id = useId();
  return (
    <section aria-labelledby={id} className={className ? `stats-section ${className}` : 'stats-section'}>
      <div className="stats-section__head">
        <h2 className="section-title" id={id}>
          {title}
        </h2>
        {aside ? <span className="stats-section__aside">{aside}</span> : null}
      </div>
      {children}
    </section>
  );
}

/** The big figure of a one-day view: value, what it is out of, and a caption under it. */
export function ScoreHero({ value, of, label, tone, text }: { value: ReactNode; of?: ReactNode; label?: ReactNode; tone?: string; /** A word or a long figure ("Productive") rather than a number: a smaller size. */ text?: boolean }) {
  return (
    <div className={text ? 'score-hero score-hero--text' : 'score-hero'}>
      <span className={tone ? `score-hero__value ${tone}` : 'score-hero__value'}>{value}</span>
      {of ? <span className="score-hero__of">{of}</span> : null}
      {label ? <span className="score-hero__label">{label}</span> : null}
    </div>
  );
}

/** Bold headline line with a paragraph under it. */
export function Headline({ title, children }: { title?: ReactNode; children: ReactNode }) {
  return (
    <div className="headline">
      {title ? <p className="headline__title">{title}</p> : null}
      <p className="headline__text">{children}</p>
    </div>
  );
}

/** A bordered row: a name and a grey line under it on the left, a rating on the right. */
export function FactorRow({ name, sub, rating, to }: { name: ReactNode; sub?: ReactNode; rating?: ReactNode; to?: string }) {
  const inner = (
    <>
      <span className="factor-row__main">
        <span className="factor-row__name">{name}</span>
        {sub ? <span className="factor-row__sub">{sub}</span> : null}
      </span>
      {rating ? <span className="factor-row__rating">{rating}</span> : null}
    </>
  );
  return <li>{to ? <Link to={to} className="factor-row">{inner}</Link> : <div className="factor-row">{inner}</div>}</li>;
}

export function FactorList({ children, tight, grid, label }: { children: ReactNode; tight?: boolean; grid?: boolean; label?: string }) {
  const cls = ['factor-list', tight && 'factor-list--tight', grid && 'factor-list--grid'].filter(Boolean).join(' ');
  return (
    <ul className={cls} aria-label={label}>
      {children}
    </ul>
  );
}

interface DayToolbarProps {
  /** The chip: "Nov 23", "November 2026". */
  label: ReactNode;
  /** Names of the arrows: "Previous day". */
  prevLabel: string;
  nextLabel: string;
  onPrev: () => void;
  onNext: () => void;
  /** No later page: the next arrow is disabled. */
  atEnd: boolean;
  /** Right of the row: links to the longer periods. */
  end?: ReactNode;
}

/** Previous and next buttons with a date chip; for pages whose period is one day or one month. */
export function DayToolbar({ label, prevLabel, nextLabel, onPrev, onNext, atEnd, end }: DayToolbarProps) {
  return (
    <div className="day-toolbar">
      <div className="day-toolbar__nav">
        <button type="button" className="round-btn" aria-label={prevLabel} onClick={onPrev}>
          <BackIcon size={18} />
        </button>
        <button type="button" className="round-btn" aria-label={nextLabel} disabled={atEnd} onClick={onNext}>
          <NextIcon size={18} />
        </button>
        <p className="date-chip" aria-live="polite">
          <CalendarIcon size={16} />
          {label}
        </p>
      </div>
      {end ? <div className="day-toolbar__end">{end}</div> : null}
    </div>
  );
}
