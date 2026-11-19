import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { BackIcon, NextIcon } from './icons';

/** Where a previous/next button goes: a route, a click handler, or nothing (end of range). */
export type NavTarget = { to: string } | { onClick: () => void } | null;

interface BaseProps {
  title: ReactNode;
  /** Use 2 when a page shows several bars (gallery); a normal page has exactly one h1. */
  headingLevel?: 1 | 2;
  /** Right-hand slot: Sample data chip, "Watch synced 07:42", a time-range control. */
  aside?: ReactNode;
}

interface LargeProps extends BaseProps {
  variant: 'large';
  /** Small line above the title, as on the canvas ("Thursday 19 November"). */
  subtitle?: ReactNode;
}
interface BackProps extends BaseProps {
  variant: 'back';
  /** Line under the title ("Night of Wed 18 Nov"). */
  subtitle?: ReactNode;
  backTo: string;
  backLabel?: string;
  /** Previous and next buttons next to the right-hand slot (Sleep: previous and next night). */
  prev?: NavTarget;
  next?: NavTarget;
  prevLabel?: string;
  nextLabel?: string;
}
interface PagerProps extends BaseProps {
  variant: 'pager';
  subtitle?: ReactNode;
  prev: NavTarget;
  next: NavTarget;
  prevLabel: string;
  nextLabel: string;
}

export type TopBarProps = LargeProps | BackProps | PagerProps;

function ArrowButton({ target, label, children }: { target: NavTarget; label: string; children: ReactNode }) {
  if (target === null) {
    return (
      <span className="icon-btn" data-disabled="true" aria-hidden="true">
        {children}
      </span>
    );
  }
  if ('to' in target) {
    return (
      <Link to={target.to} className="icon-btn" aria-label={label}>
        {children}
      </Link>
    );
  }
  return (
    <button type="button" className="icon-btn" aria-label={label} onClick={target.onClick}>
      {children}
    </button>
  );
}

/**
 * Page header in three variants from the canvas:
 * `large` (big title, subtitle above), `back` (back button, title, subtitle below) and
 * `pager` (previous and next buttons around a centred title).
 * The title is the page's single h1.
 */
export function TopBar(props: TopBarProps) {
  const Heading = props.headingLevel === 2 ? 'h2' : 'h1';
  if (props.variant === 'large') {
    return (
      <header className="topbar topbar--large">
        <div className="topbar__text">
          {props.subtitle ? <span className="topbar__eyebrow">{props.subtitle}</span> : null}
          <Heading className="topbar__title">{props.title}</Heading>
        </div>
        {props.aside ? <div className="topbar__aside">{props.aside}</div> : null}
      </header>
    );
  }
  if (props.variant === 'back') {
    return (
      <header className="topbar topbar--back">
        <Link to={props.backTo} className="icon-btn" aria-label={props.backLabel ?? 'Back'}>
          <BackIcon />
        </Link>
        <div className="topbar__text">
          <Heading className="topbar__title">{props.title}</Heading>
          {props.subtitle ? <span className="topbar__sub">{props.subtitle}</span> : null}
        </div>
        {props.aside || props.prev !== undefined || props.next !== undefined ? (
          <div className="topbar__aside">
            {props.aside}
            {props.prev !== undefined ? (
              <ArrowButton target={props.prev} label={props.prevLabel ?? 'Previous'}>
                <BackIcon />
              </ArrowButton>
            ) : null}
            {props.next !== undefined ? (
              <ArrowButton target={props.next} label={props.nextLabel ?? 'Next'}>
                <NextIcon />
              </ArrowButton>
            ) : null}
          </div>
        ) : null}
      </header>
    );
  }
  return (
    <header className="topbar topbar--pager">
      <ArrowButton target={props.prev} label={props.prevLabel}>
        <BackIcon />
      </ArrowButton>
      <div className="topbar__text">
        <Heading className="topbar__title">{props.title}</Heading>
        {props.subtitle ? <span className="topbar__sub">{props.subtitle}</span> : null}
      </div>
      <ArrowButton target={props.next} label={props.nextLabel}>
        <NextIcon />
      </ArrowButton>
    </header>
  );
}
