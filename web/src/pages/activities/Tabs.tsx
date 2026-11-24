import { useId, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import './activities.css';

export interface TabDef {
  id: string;
  label: string;
  content: ReactNode;
  disabled?: boolean;
}

/**
 * Tabs under the charts (Stats, Laps, Time in Zones): a tab list with one panel. Left and right
 * arrows, Home and End move between tabs and select them; Tab goes on into the panel.
 */
export function Tabs({ label, tabs, value, onChange }: { label: string; tabs: readonly TabDef[]; value?: string; onChange?: (id: string) => void }) {
  const base = useId();
  const [inner, setInner] = useState(tabs[0]?.id ?? '');
  const active = value ?? inner;
  const list = useRef<HTMLDivElement>(null);
  const select = (id: string) => {
    setInner(id);
    onChange?.(id);
  };
  const current = tabs.find((t) => t.id === active) ?? tabs[0];

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    const enabled = tabs.filter((t) => !t.disabled);
    const at = enabled.findIndex((t) => t.id === current?.id);
    let next = -1;
    if (e.key === 'ArrowRight') next = (at + 1) % enabled.length;
    else if (e.key === 'ArrowLeft') next = (at - 1 + enabled.length) % enabled.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = enabled.length - 1;
    if (next < 0) return;
    e.preventDefault();
    const target = enabled[next];
    if (!target) return;
    select(target.id);
    list.current?.querySelector<HTMLButtonElement>(`[data-tab="${target.id}"]`)?.focus();
  };

  return (
    <div className="adt-tabs">
      <div ref={list} role="tablist" aria-label={label} className="adt-tabs__list">
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            id={`${base}-tab-${t.id}`}
            data-tab={t.id}
            aria-selected={t.id === current?.id}
            aria-controls={`${base}-panel-${t.id}`}
            tabIndex={t.id === current?.id ? 0 : -1}
            disabled={t.disabled}
            onClick={() => select(t.id)}
            onKeyDown={onKeyDown}
          >
            {t.label}
          </button>
        ))}
      </div>
      {current ? (
        <div role="tabpanel" id={`${base}-panel-${current.id}`} aria-labelledby={`${base}-tab-${current.id}`} className="adt-tabs__panel">
          {current.content}
        </div>
      ) : null}
    </div>
  );
}
