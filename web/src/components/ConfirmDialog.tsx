import type { ReactNode } from 'react';
import { Button } from './Button';
import { Modal } from './Modal';

interface ConfirmDialogProps {
  /** A question that names the action ("Log out?", "Sign out Pixel 9a?"). */
  title: ReactNode;
  /** What happens when the person says yes, in plain words. Also the dialog's accessible description. */
  consequence: ReactNode;
  /** The yes button names the action too ("Log out"), never just "OK". */
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  /** The request is running: the buttons wait. */
  pending?: boolean;
  /** Why the last try failed; the dialog stays open so the person can try again. */
  error?: ReactNode;
}

/**
 * "Are you sure?" for the things that sign something out or switch something off. A `Modal`
 * underneath, so focus moves in, Tab stays inside, Escape and a tap outside cancel, and focus
 * returns to the row that opened it. The consequence is stated before the buttons.
 */
export function ConfirmDialog({ title, consequence, confirmLabel, onConfirm, onCancel, pending = false, error }: ConfirmDialogProps) {
  return (
    <Modal
      title={title}
      description={consequence}
      onClose={() => {
        if (!pending) onCancel();
      }}
      footer={
        <>
          <Button size="lg" onClick={onCancel} disabled={pending}>
            Cancel
          </Button>
          <Button size="lg" variant="primary" onClick={onConfirm} disabled={pending} aria-busy={pending}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {error ? (
        <p className="status-msg status-msg--error" role="alert">
          {error}
        </p>
      ) : null}
    </Modal>
  );
}
