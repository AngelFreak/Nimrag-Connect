import type { CSSProperties } from 'react';
import { Unavailable } from './Unavailable';

export interface ZoneEntry {
  zone: 1 | 2 | 3 | 4 | 5;
  /** Minutes in the zone; null when the activity has no heart-rate data. */
  minutes: number | null | undefined;
}

export interface BarListItem {
  key: string;
  label: string;
  /** null renders "unavailable" instead of a bar. */
  value: number | null | undefined;
  /** Text at the end of the row ("412"). */
  text?: string;
  color: string;
}

/** Labelled horizontal bars (Training: load by intensity). Values share one scale. */
export function BarList({ items, label, labelWidth = 96, max }: { items: BarListItem[]; label: string; labelWidth?: number; max?: number }) {
  const top = max ?? Math.max(1, ...items.map((i) => (typeof i.value === 'number' && Number.isFinite(i.value) ? i.value : 0)));
  return (
    <ul className="zones" aria-label={label}>
      {items.map((it) => {
        const missing = it.value === null || it.value === undefined || !Number.isFinite(it.value);
        return (
          <li className="zones__row" key={it.key} style={{ gridTemplateColumns: `minmax(${labelWidth}px, max-content) 1fr minmax(40px, auto)` }}>
            <span>{it.label}</span>
            <div className="progress" style={{ '--h': '10px' } as CSSProperties} aria-hidden="true">
              <div className="progress__fill" style={{ width: missing ? 0 : `${((it.value as number) / top) * 100}%`, background: it.color }} />
            </div>
            <span className="zones__value">{missing ? <Unavailable /> : (it.text ?? it.value)}</span>
          </li>
        );
      })}
    </ul>
  );
}

/** Time in heart-rate zones, zone 5 on top. The word "Zone N" always sits beside the bar. */
export function ZoneBars({ zones }: { zones: ZoneEntry[] }) {
  const sorted = [...zones].sort((a, b) => b.zone - a.zone);
  const total = sorted.reduce((a, z) => a + (typeof z.minutes === 'number' && Number.isFinite(z.minutes) ? z.minutes : 0), 0);
  return (
    <ul className="zones" aria-label="Time in heart rate zones">
      {sorted.map((z) => {
        const missing = z.minutes === null || z.minutes === undefined || !Number.isFinite(z.minutes);
        const pct = missing || total <= 0 ? 0 : ((z.minutes as number) / total) * 100;
        return (
          <li className="zones__row" key={z.zone}>
            <span>Zone {z.zone}</span>
            <div className="progress" style={{ '--h': '10px' } as CSSProperties} aria-hidden="true">
              <div className="progress__fill" style={{ width: `${pct}%`, background: `var(--zone-${z.zone})` }} />
            </div>
            <span className="zones__value">{missing ? <Unavailable /> : `${z.minutes} min`}</span>
          </li>
        );
      })}
    </ul>
  );
}
