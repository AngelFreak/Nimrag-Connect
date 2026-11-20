import { useEffect, useId, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { useLogout, useMe } from '../account/queries';
import { UserIcon } from './icons';

/**
 * The profile button of the top bar. It opens a small panel with who is signed in (username and
 * role), a link to Settings (people with data) or to the admin area (admins), and Log out.
 * A disclosure rather than an ARIA menu: every item is an ordinary link or button in tab order.
 */
export function ProfileMenu({ align = 'start' }: { align?: 'start' | 'end' }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  // Only asks the server once the panel is open; the route guard has normally cached the answer already.
  const me = useMe(open);
  const logout = useLogout();
  const who = me.data;

  useEffect(() => {
    if (!open) return;
    const onPointer = (e: PointerEvent) => {
      if (root.current && e.target instanceof Node && !root.current.contains(e.target)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      setOpen(false);
      button.current?.focus();
    };
    document.addEventListener('pointerdown', onPointer);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('pointerdown', onPointer);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div
      className="profile-menu"
      ref={root}
      onBlur={(e) => {
        // Tabbing out of the panel closes it.
        if (open && e.relatedTarget instanceof Node && !e.currentTarget.contains(e.relatedTarget)) setOpen(false);
      }}
    >
      <button ref={button} type="button" className="icon-btn" aria-label="Profile and account" aria-expanded={open} aria-controls={open ? panelId : undefined} onClick={() => setOpen((o) => !o)}>
        <UserIcon />
      </button>
      {open ? (
        <div id={panelId} className="profile-menu__panel" data-align={align} role="group" aria-label="Account">
          {who ? (
            <p className="profile-menu__who">
              <span className="profile-menu__name">{who.username}</span>
              <span className="profile-menu__role">{who.role === 'admin' ? 'Admin' : 'User'}</span>
            </p>
          ) : null}
          {who?.role === 'admin' ? (
            <Link to="/admin" className="profile-menu__item" onClick={() => setOpen(false)}>
              Admin area
            </Link>
          ) : (
            <Link to="/settings" className="profile-menu__item" onClick={() => setOpen(false)}>
              Settings
            </Link>
          )}
          <button type="button" className="profile-menu__item" onClick={() => logout.mutate()} disabled={logout.isPending}>
            Log out
          </button>
        </div>
      ) : null}
    </div>
  );
}
