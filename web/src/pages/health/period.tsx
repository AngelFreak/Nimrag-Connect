import { useCallback } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useMe } from '../../account/queries';
import { BackIcon, NextIcon, SegmentedControl } from '../../components';
import { addDays, fmtPeriod, isIsoDay, todayIn } from '../../lib/dates';
import { useIsDesktop } from '../../lib/hooks';

// The period picker the Training status, HRV status and Sleep score pages share: a segmented control
// (the length of the period), and under it the dates with a back arrow that steps one period
// back. The period lives in the address (`?range=4w&end=2026-11-13`), so it can be linked and the
// back button works. A period ends on `end` (default: today) and the forward arrow stops at today.

export interface PeriodOption<K extends string> {
  value: K;
  label: string;
  /** Length in days. */
  days: number;
}

export interface Period<K extends string> {
  option: PeriodOption<K>;
  from: string;
  to: string;
  today: string;
  atToday: boolean;
  setRange: (range: K) => void;
  step: (direction: -1 | 1) => void;
}

/** Pure part of the period: the window of `days` days ending on `end` (never after `today`). */
export function periodWindow(end: string | null, days: number, today: string): { from: string; to: string } {
  const to = isIsoDay(end ?? undefined) && (end as string) < today ? (end as string) : today;
  return { from: addDays(to, -(days - 1)), to };
}

export function usePeriod<K extends string>(options: readonly PeriodOption<K>[], fallback: K): Period<K> {
  const [params, setParams] = useSearchParams();
  const me = useMe();
  const today = todayIn(me.data?.timezone);
  const option = options.find((o) => o.value === params.get('range')) ?? (options.find((o) => o.value === fallback) as PeriodOption<K>);
  const { from, to } = periodWindow(params.get('end'), option.days, today);

  const go = useCallback(
    (range: string, end: string) => {
      const next = new URLSearchParams();
      next.set('range', range);
      if (end < today) next.set('end', end);
      setParams(next);
    },
    [setParams, today],
  );
  return {
    option,
    from,
    to,
    today,
    atToday: to >= today,
    setRange: (range) => go(range, today),
    step: (direction) => go(option.value, direction < 0 ? addDays(to, -option.days) : addDays(to, option.days) > today ? today : addDays(to, option.days)),
  };
}

interface RangeBarProps<K extends string> {
  label: string;
  options: readonly PeriodOption<K>[];
  value: K;
  onChange: (value: K) => void;
}

/** The segmented control, laid out like the one on the Health page (full row on a phone). */
export function RangeBar<K extends string>({ label, options, value, onChange }: RangeBarProps<K>) {
  const desktop = useIsDesktop();
  return (
    <div className="range-bar">
      <SegmentedControl label={label} options={options} value={value} onChange={onChange} layout={desktop ? 'inline' : 'grid'} />
    </div>
  );
}

/** "Nov 14 – Nov 20" with arrows; the forward arrow is disabled when the period ends today. */
export function PeriodNav({ from, to, atToday, onStep, unit }: { from: string; to: string; atToday: boolean; onStep: (direction: -1 | 1) => void; unit: string }) {
  return (
    <div className="period-nav">
      <button type="button" className="icon-btn" aria-label={`Previous ${unit}`} onClick={() => onStep(-1)}>
        <BackIcon />
      </button>
      <p className="period-nav__dates" aria-live="polite">
        {fmtPeriod(from, to)}
      </p>
      <button type="button" className="icon-btn" aria-label={`Next ${unit}`} disabled={atToday} data-disabled={atToday ? 'true' : undefined} onClick={() => onStep(1)}>
        <NextIcon />
      </button>
    </div>
  );
}
