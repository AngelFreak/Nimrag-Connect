import type { CSSProperties } from 'react';

interface ProgressBarProps {
  /** null / undefined renders an empty track; pair it with an <Unavailable /> caption. */
  value: number | null | undefined;
  max: number;
  /** Accessible name including the numbers: "6,240 of 8,000 steps". */
  label: string;
  color?: string;
  /** Track height in px: 6 (tile), 8 (card), 10 (zones). */
  height?: 6 | 8 | 10 | 12;
}

/** Single-value progress bar (steps toward the goal). */
export function ProgressBar({ value, max, label, color = 'var(--steps)', height = 6 }: ProgressBarProps) {
  const missing = value === null || value === undefined || !Number.isFinite(value);
  const pct = missing || max <= 0 ? 0 : Math.min(100, Math.max(0, (value / max) * 100));
  return (
    <div
      className="progress"
      style={{ '--h': `${height}px` } as CSSProperties}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      {...(missing ? { 'aria-valuetext': 'unavailable' } : { 'aria-valuenow': value })}
    >
      <div className="progress__fill" style={{ width: `${pct}%`, background: color }} />
    </div>
  );
}

interface RangeBarProps {
  value: number | null | undefined;
  /** Axis range the bar maps onto. */
  min?: number;
  max: number;
  /** Target range drawn as a band, e.g. 240 to 420. */
  target: [number, number];
  label: string;
}

/** Value marker over a target band (Training: 7-day load against its target range). */
export function RangeBar({ value, min = 0, max, target, label }: RangeBarProps) {
  const span = max - min;
  const pos = (n: number) => `${Math.min(100, Math.max(0, ((n - min) / span) * 100))}%`;
  const missing = value === null || value === undefined || !Number.isFinite(value);
  return (
    <div className="progress progress--range" style={{ '--h': '12px' } as CSSProperties} role="img" aria-label={label}>
      <div className="progress__range" style={{ left: pos(target[0]), width: `calc(${pos(target[1])} - ${pos(target[0])})` }} />
      {missing ? null : <div className="progress__marker" style={{ left: `calc(${pos(value)} - 2px)` }} />}
    </div>
  );
}
