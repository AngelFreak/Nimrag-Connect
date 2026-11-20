import { useEffect, useId, useRef, useState, type DragEvent, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import type { HomeCard, HomeCards, SettingsResponse } from '../../api/generated';
import { Button } from '../../components';
import { CARD_LABEL, DEFAULT_HOME_CARDS } from './cards';
import { useSaveHomeCards } from './queries';

const FOCUSABLE = 'button:not(:disabled), input:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])';

function ChevronIcon({ up }: { up: boolean }) {
  return (
    <svg width={22} height={22} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
      <path d={up ? 'M6 15l6-6 6 6' : 'M6 9l6 6 6-6'} />
    </svg>
  );
}

function GripIcon() {
  return (
    <svg width={18} height={18} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" focusable="false">
      {[7, 12, 17].flatMap((y) => [9, 15].map((x) => <circle key={`${x}-${y}`} cx={x} cy={y} r={1.6} />))}
    </svg>
  );
}

export function moveCard(order: HomeCard[], id: HomeCard, to: number): HomeCard[] {
  const from = order.indexOf(id);
  if (from < 0 || to < 0 || to >= order.length || from === to) return order;
  const next = order.slice();
  next.splice(from, 1);
  next.splice(to, 0, id);
  return next;
}

interface SheetProps {
  initial: HomeCards;
  error: string | null;
  onSave: (cards: HomeCards) => void;
  onClose: () => void;
}

/**
 * The card picker. A modal dialog: focus moves into it, Tab stays inside, Escape closes it and
 * focus goes back to the button that opened it. Rows reorder by drag and drop or with the
 * move up and move down buttons.
 */
export function EditHomeSheet({ initial, error, onSave, onClose }: SheetProps) {
  const titleId = useId();
  const hintId = useId();
  const dialog = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const pendingFocus = useRef<{ id: HomeCard; dir: 'up' | 'down' } | null>(null);
  const [order, setOrder] = useState<HomeCard[]>(initial.order);
  const [hidden, setHidden] = useState<HomeCard[]>(initial.hidden);
  const [dragging, setDragging] = useState<HomeCard | null>(null);
  const [over, setOver] = useState<HomeCard | null>(null);
  const [announcement, setAnnouncement] = useState('');

  // Focus into the dialog on open, back to the opener on close; lock page scroll meanwhile.
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.focus();
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = previousOverflow;
      opener?.focus();
    };
  }, []);

  // After a keyboard move, keep focus on the same button (it has moved with its row).
  useEffect(() => {
    const target = pendingFocus.current;
    if (!target) return;
    pendingFocus.current = null;
    const row = list.current?.querySelector<HTMLElement>(`[data-card="${target.id}"]`);
    const preferred = row?.querySelector<HTMLButtonElement>(`[data-dir="${target.dir}"]`);
    const other = row?.querySelector<HTMLButtonElement>(`[data-dir="${target.dir === 'up' ? 'down' : 'up'}"]`);
    (preferred && !preferred.disabled ? preferred : other)?.focus();
  }, [order]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key !== 'Tab' || !dialog.current) return;
    const items = Array.from(dialog.current.querySelectorAll<HTMLElement>(FOCUSABLE));
    if (items.length === 0) return;
    const first = items[0]!;
    const last = items[items.length - 1]!;
    const active = document.activeElement;
    if (e.shiftKey && (active === first || active === dialog.current)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  };

  const move = (id: HomeCard, to: number, dir: 'up' | 'down' | null) => {
    const next = moveCard(order, id, to);
    if (next === order) return;
    if (dir) pendingFocus.current = { id, dir };
    setOrder(next);
    setAnnouncement(`${CARD_LABEL[id]} moved to position ${to + 1} of ${next.length}`);
  };

  const toggle = (id: HomeCard) => {
    const nowHidden = !hidden.includes(id);
    setHidden(nowHidden ? [...hidden, id] : hidden.filter((h) => h !== id));
    setAnnouncement(`${CARD_LABEL[id]} ${nowHidden ? 'hidden' : 'shown'}`);
  };

  const onDragStart = (e: DragEvent<HTMLLIElement>, id: HomeCard) => {
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', id);
    setDragging(id);
  };
  const onDragOver = (e: DragEvent<HTMLLIElement>, id: HomeCard) => {
    if (!dragging) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    if (over !== id) setOver(id);
  };
  const onDrop = (e: DragEvent<HTMLLIElement>, id: HomeCard) => {
    e.preventDefault();
    if (dragging && dragging !== id) move(dragging, order.indexOf(id), null);
    setDragging(null);
    setOver(null);
  };
  const endDrag = () => {
    setDragging(null);
    setOver(null);
  };

  const reset = () => {
    setOrder(DEFAULT_HOME_CARDS.order);
    setHidden(DEFAULT_HOME_CARDS.hidden);
    setAnnouncement('Reset to the default layout');
  };

  const save = () => onSave({ order, hidden: order.filter((id) => hidden.includes(id)) });

  return createPortal(
    <div
      className="sheet-backdrop"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- the dialog handles Escape and the Tab focus trap */}
      <div ref={dialog} className="sheet" role="dialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={hintId} tabIndex={-1} onKeyDown={onKeyDown}>
        <div className="sheet__head">
          <h2 className="sheet__title" id={titleId}>
            Edit home
          </h2>
        </div>
        <p className="sheet__hint" id={hintId}>
          Choose which cards appear under At a glance and in what order. Drag a row, or use the arrow buttons.
        </p>
        {error ? (
          <p className="today-alert sheet__error" role="alert">
            {error}
          </p>
        ) : null}
        <div className="sheet__body" data-scroll="y">
          <ul ref={list} className="edit-list" aria-label="Home cards">
            {order.map((id, i) => {
              const shown = !hidden.includes(id);
              return (
                <li
                  key={id}
                  className="edit-row"
                  data-card={id}
                  data-dragging={dragging === id}
                  data-over={over === id && dragging !== id}
                  draggable
                  onDragStart={(e) => onDragStart(e, id)}
                  onDragOver={(e) => onDragOver(e, id)}
                  onDrop={(e) => onDrop(e, id)}
                  onDragEnd={endDrag}
                >
                  <span className="edit-row__grip" aria-hidden="true" title="Drag to reorder">
                    <GripIcon />
                  </span>
                  <label className="edit-row__label">
                    <input type="checkbox" checked={shown} onChange={() => toggle(id)} />
                    <span>
                      <span className="edit-row__name">{CARD_LABEL[id]}</span>
                      {shown ? null : <span className="edit-row__state"> (hidden)</span>}
                    </span>
                  </label>
                  <span className="edit-row__moves">
                    <button type="button" className="icon-btn" data-dir="up" aria-label={`Move ${CARD_LABEL[id]} up`} disabled={i === 0} onClick={() => move(id, i - 1, 'up')}>
                      <ChevronIcon up />
                    </button>
                    <button type="button" className="icon-btn" data-dir="down" aria-label={`Move ${CARD_LABEL[id]} down`} disabled={i === order.length - 1} onClick={() => move(id, i + 1, 'down')}>
                      <ChevronIcon up={false} />
                    </button>
                  </span>
                </li>
              );
            })}
          </ul>
        </div>
        <p className="sr-only" role="status" aria-live="polite">
          {announcement}
        </p>
        <div className="sheet__foot">
          <Button onClick={reset}>Reset to default</Button>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={save}>
            Save
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

interface EditHomeProps {
  /** `undefined` while the settings are loading or failed: the button stays disabled. */
  settings: SettingsResponse | undefined;
  /** Full-width button (phone). */
  block?: boolean;
}

/** The "Edit home" button, its sheet, and the optimistic save with rollback on error. */
export function EditHome({ settings, block = false }: EditHomeProps) {
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<HomeCards | null>(null);
  const mutation = useSaveHomeCards();

  const onSave = (cards: HomeCards) => {
    const current = settings?.home_cards;
    if (current && JSON.stringify(current) === JSON.stringify(cards)) {
      setOpen(false);
      return;
    }
    setError(null);
    setDraft(null);
    setOpen(false);
    mutation.mutate(cards, {
      onError: (e) => {
        setDraft(cards);
        setError(`Could not save your layout${e.message ? `: ${e.message}` : ''}. The previous layout is back; your changes are still here to try again.`);
        setOpen(true);
      },
    });
  };

  return (
    <>
      <Button block={block} onClick={() => setOpen(true)} disabled={!settings}>
        Edit home
      </Button>
      {open && settings ? (
        <EditHomeSheet
          initial={draft ?? settings.home_cards}
          error={error}
          onSave={onSave}
          onClose={() => {
            setOpen(false);
            setError(null);
            setDraft(null);
          }}
        />
      ) : null}
    </>
  );
}
