import type { ReactNode } from 'react';
import { fmtPeriod } from '../lib/dates';
import { CalendarIcon, NextIcon, BackIcon } from './icons';
import { SegmentedControl } from './SegmentedControl';
import { isMissing } from '../lib/format';
import { ValueOrUnavailable } from './Unavailable';

// The header of a stats page (pattern A in docs/design/web-layout/SPEC.md): the page title, a toolbar
// with the period arrows, the dates and the range control, and a rule under it. `PeriodToolbar` takes the
// props that `usePeriod` (health/period.tsx) and `useMetricPeriod` (metrics/MetricView.tsx) already hold.

interface PageHeaderProps {
  /** The page's title (an h1; `headingLevel` 2 where a page shows several headers, as the gallery does). */
  title: ReactNode;
  headingLevel?: 1 | 2;
  /** Right of the title: icon buttons, a sample-data chip. */
  actions?: ReactNode;
  /** The toolbar row (a `PeriodToolbar`); a rule is drawn under it. */
  children?: ReactNode;
}

export function PageHeader({ title, headingLevel = 1, actions, children }: PageHeaderProps) {
  const Heading = headingLevel === 1 ? 'h1' : 'h2';
  return (
    <div className={children ? 'page-header' : 'page-header page-header--bare'}>
      <div className="page-header__row">
        <Heading className="page-title">{title}</Heading>
        {actions ? <div className="page-header__actions">{actions}</div> : null}
      </div>
      {children}
      {children ? <hr className="page-header__rule" /> : null}
    </div>
  );
}

/** The labels of the range control: the keys the metric and health pages use for a period length. */
export const RANGE_LABELS: Record<string, string> = {
  '1d': '1 Day',
  '7d': '7 Days',
  '4w': '4 Weeks',
  '12w': '12 Weeks',
  '6m': '6 Months',
  '1y': '1 Year',
};

/** "7 Days" for `7d`; a key without a long label is shown as it is. */
export const rangeLabel = (key: string): string => RANGE_LABELS[key] ?? key;

interface PeriodToolbarProps<K extends string> {
  /** First and last day of the period ("YYYY-MM-DD"); the date chip shows "Nov 18 – Nov 24". */
  from: string;
  to: string;
  /** The period reaches today: the next arrow is disabled. */
  atToday: boolean;
  /** One period back (-1) or forward (+1); `Period.step` and `MetricPeriod.step`. */
  onStep: (direction: -1 | 1) => void;
  /** What an arrow moves by, for its name: "day", "week", "period" ("Previous week"). */
  unit: string;
  /** The range control; `PeriodOption` (health/period.tsx) fits. Label each with `rangeLabel(key)`. */
  options: readonly { value: K; label: string }[];
  value: K;
  onChange: (value: K) => void;
  /** Accessible name of the range control. */
  label?: string;
}

/** Previous and next round buttons, a date chip and the segmented range control, in one row. */
export function PeriodToolbar<K extends string>({ from, to, atToday, onStep, unit, options, value, onChange, label = 'Period' }: PeriodToolbarProps<K>) {
  return (
    <div className="period-toolbar">
      <div className="period-toolbar__nav">
        <button type="button" className="round-btn" aria-label={`Previous ${unit}`} onClick={() => onStep(-1)}>
          <BackIcon size={18} />
        </button>
        <button type="button" className="round-btn" aria-label={`Next ${unit}`} disabled={atToday} onClick={() => onStep(1)}>
          <NextIcon size={18} />
        </button>
        <p className="date-chip" aria-live="polite">
          <CalendarIcon size={16} />
          {fmtPeriod(from, to)}
        </p>
      </div>
      <SegmentedControl label={label} options={options} value={value} onChange={onChange} />
    </div>
  );
}

export interface StatStripItem {
  key: string;
  label: ReactNode;
  /** Pre-formatted value with its unit; a cell with no value is left out. */
  value: ReactNode | null | undefined;
}

/** Figures in a row under a chart: value 24/300 on top, label under it, each cell under a rule. Cells without a value are left out. */
export function StatStrip({ items }: { items: readonly StatStripItem[] }) {
  const shown = items.filter((item) => !(isMissing(item.value) || item.value === '' || item.value === false));
  if (shown.length === 0) return null;
  return (
    <dl className="stat-strip">
      {shown.map((item) => (
        <div key={item.key} className="stat-strip__cell">
          <dt className="stat-strip__label">{item.label}</dt>
          <dd className="stat-strip__value">
            <ValueOrUnavailable value={item.value} />
          </dd>
        </div>
      ))}
    </dl>
  );
}
