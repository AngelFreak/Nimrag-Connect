import type { ReactNode } from 'react';

interface RingProps {
  /** Current value; null / undefined shows the ring empty with "unavailable" inside. */
  value: number | null | undefined;
  max?: number;
  /** What the ring measures, used in the accessible name ("Energy level"). */
  label: string;
  color?: string;
  size?: number;
  strokeWidth?: number;
  /** Text inside the ring; defaults to the value. */
  children?: ReactNode;
  /** Set false for a ring without a centre number (Training recovery). */
  showValue?: boolean;
}

/** Circular progress ring, as the Today energy ring and the Training recovery ring. */
export function Ring({ value, max = 100, label, color = 'var(--energy)', size = 104, strokeWidth = 10, children, showValue = true }: RingProps) {
  const r = (size - strokeWidth) / 2 - 2;
  const c = 2 * Math.PI * r;
  const missing = value === null || value === undefined || !Number.isFinite(value);
  const frac = missing ? 0 : Math.min(1, Math.max(0, value / max));
  const mid = size / 2;
  const aria = missing ? `${label} unavailable` : `${label} ${value} out of ${max}`;
  return (
    <svg className="ring" width={size} height={size} viewBox={`0 0 ${size} ${size}`} role="img" aria-label={aria}>
      <circle cx={mid} cy={mid} r={r} fill="none" style={{ stroke: 'var(--track)' }} strokeWidth={strokeWidth} />
      {!missing && frac > 0 ? (
        <circle
          cx={mid}
          cy={mid}
          r={r}
          fill="none"
          style={{ stroke: color }}
          strokeWidth={strokeWidth}
          strokeLinecap="round"
          strokeDasharray={`${(c * frac).toFixed(1)} ${c.toFixed(1)}`}
          transform={`rotate(-90 ${mid} ${mid})`}
        />
      ) : null}
      {missing ? (
        <text
          x={mid}
          y={mid + 4}
          textAnchor="middle"
          fontFamily="var(--font-body)"
          fontStyle="italic"
          fontSize={Math.max(11, size * 0.12)}
          style={{ fill: 'var(--text-2)' }}
        >
          unavailable
        </text>
      ) : showValue ? (
        <text x={mid} y={mid + size * 0.08} textAnchor="middle" fontFamily="var(--font-display)" fontWeight={600} fontSize={size * 0.29} style={{ fill: 'var(--text)' }}>
          {children ?? value}
        </text>
      ) : null}
    </svg>
  );
}
