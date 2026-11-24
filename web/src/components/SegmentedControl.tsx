import { useEffect, useRef, type CSSProperties } from 'react';

export interface Option<V extends string> {
  value: V;
  label: string;
  /** A shorter word drawn instead of `label` where space is tight ("HR"); the full label stays the accessible name. */
  short?: string;
}

/**
 * Arrow-key navigation inside a button group: arrows, Home and End move focus between the
 * buttons (they do not select). Attach the returned ref to the group element.
 */
export function useGroupArrows<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onKeyDown = (e: KeyboardEvent) => {
      const keys = ['ArrowRight', 'ArrowLeft', 'ArrowDown', 'ArrowUp', 'Home', 'End'];
      if (!keys.includes(e.key)) return;
      const buttons = Array.from(el.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
      const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
      if (current < 0) return;
      let next = current;
      if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (current + 1) % buttons.length;
      else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = (current - 1 + buttons.length) % buttons.length;
      else if (e.key === 'Home') next = 0;
      else if (e.key === 'End') next = buttons.length - 1;
      e.preventDefault();
      buttons[next]?.focus();
    };
    el.addEventListener('keydown', onKeyDown);
    return () => el.removeEventListener('keydown', onKeyDown);
  }, []);
  return ref;
}

interface SegmentedControlProps<V extends string> {
  /** Accessible name of the group ("Time range"). */
  label: string;
  options: readonly Option<V>[];
  value: V;
  onChange: (value: V) => void;
  /** "grid" stretches the options over the full row (phone Health page). */
  layout?: 'inline' | 'grid';
}

/**
 * Time-range switch. A group of toggle buttons (aria-pressed) as drawn on the canvas:
 * Tab reaches every option, Enter or Space selects, arrow keys move focus.
 */
export function SegmentedControl<V extends string>({ label, options, value, onChange, layout = 'inline' }: SegmentedControlProps<V>) {
  const ref = useGroupArrows<HTMLDivElement>();
  const cls = layout === 'grid' ? 'segmented segmented--grid' : 'segmented';
  return (
    <div ref={ref} role="group" aria-label={label} className={cls} style={{ '--cols': options.length } as CSSProperties}>
      {options.map((o) => (
        <button key={o.value} type="button" aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
          {o.short ? (
            <>
              <span aria-hidden="true">{o.short}</span>
              <span className="sr-only">{o.label}</span>
            </>
          ) : (
            o.label
          )}
        </button>
      ))}
    </div>
  );
}
