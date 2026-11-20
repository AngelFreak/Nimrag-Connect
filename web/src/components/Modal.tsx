import { useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

const FOCUSABLE = 'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])';

interface ModalProps {
  title: ReactNode;
  /** Short explanation under the title; also the dialog's accessible description. */
  description?: ReactNode;
  /** Escape and a click on the backdrop call this. */
  onClose: () => void;
  /** Buttons along the bottom edge. */
  footer?: ReactNode;
  children?: ReactNode;
}

/**
 * A modal dialog on the same sheet surface as "Edit home" (bottom sheet on a phone, centred card
 * from 1024 px): focus moves into it, Tab stays inside, Escape closes it and focus goes back to
 * the control that opened it. The `.sheet` styles live in pages/today/today.css.
 */
export function Modal({ title, description, onClose, footer, children }: ModalProps) {
  const titleId = useId();
  const descId = useId();
  const dialog = useRef<HTMLDivElement>(null);
  // The latest onClose without re-running the focus effect when a parent re-renders.
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.focus();
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = previousOverflow;
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      close.current();
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

  return createPortal(
    <div
      className="sheet-backdrop"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close.current();
      }}
    >
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- the dialog handles Escape and the Tab focus trap */}
      <div ref={dialog} className="sheet" role="dialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={description ? descId : undefined} tabIndex={-1} onKeyDown={onKeyDown}>
        <div className="sheet__head">
          <h2 className="sheet__title" id={titleId}>
            {title}
          </h2>
        </div>
        {description ? (
          <p className="sheet__hint" id={descId}>
            {description}
          </p>
        ) : null}
        <div className="sheet__body modal__body">{children}</div>
        {footer ? <div className="sheet__foot">{footer}</div> : null}
      </div>
    </div>,
    document.body,
  );
}
