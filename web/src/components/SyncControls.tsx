import { useCallback, useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { getBridge, hasNativeSync, type LinkState } from '../native';
import { useNativeStatus } from '../lib/hooks';
import { RefreshIcon, UserIcon, WatchIcon } from './icons';

const MIN_SPIN_MS = 700;

/**
 * Sync behaviour of the top bar.
 * In the Android app (`window.NimragApp.syncNow` exists) it asks the app to sync with the
 * watch. In a normal browser it refetches every query from the server.
 */
export function useSync(): { sync: () => void; spinning: boolean; native: boolean } {
  const queryClient = useQueryClient();
  const [spinning, setSpinning] = useState(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const native = hasNativeSync();

  const sync = useCallback(() => {
    setSpinning(true);
    const minimum = new Promise<void>((resolve) => window.setTimeout(resolve, MIN_SPIN_MS));
    const bridge = getBridge();
    const work = (async () => {
      if (typeof bridge?.syncNow === 'function') {
        try {
          await bridge.syncNow();
        } catch {
          // The app reports sync problems through the watch status.
        }
      } else {
        await queryClient.invalidateQueries();
      }
    })();
    void Promise.allSettled([work, minimum]).then(() => {
      if (alive.current) setSpinning(false);
    });
  }, [queryClient]);

  return { sync, spinning, native };
}

interface SyncButtonProps {
  spinning: boolean;
  /** True when the Android app does the sync; changes the accessible name. */
  native?: boolean;
  onClick: () => void;
}

/** Sync / refresh icon button with idle and spinning states. */
export function SyncButton({ spinning, native = false, onClick }: SyncButtonProps) {
  return (
    <button
      type="button"
      className="icon-btn"
      aria-label={native ? 'Sync now' : 'Refresh'}
      aria-busy={spinning}
      data-spinning={spinning}
      onClick={onClick}
    >
      <RefreshIcon />
    </button>
  );
}

const LINK_TEXT: Record<LinkState, string> = {
  connected: 'connected',
  waiting: 'waiting to sync',
  problem: 'connection problem',
  unknown: 'status unknown',
};

/** Watch icon that links to Devices, with a status dot. State is also written in the aria-label. */
export function WatchLink({ link }: { link: LinkState }) {
  const text = `Watch ${LINK_TEXT[link]}`;
  return (
    <Link to="/devices" className="icon-btn" aria-label={text} title={text}>
      <WatchIcon />
      <span className="status-dot" data-state={link} aria-hidden="true" />
    </Link>
  );
}

/** User icon: profile and settings. */
export function ProfileLink() {
  return (
    <Link to="/settings" className="icon-btn" aria-label="Profile and settings">
      <UserIcon />
    </Link>
  );
}

/** Sync button and watch link wired to the real behaviour and status. */
export function SyncAndWatch() {
  const { sync, spinning, native } = useSync();
  const { link } = useNativeStatus();
  return (
    <>
      <SyncButton spinning={spinning} native={native} onClick={sync} />
      <WatchLink link={link} />
    </>
  );
}

/** Top bar of the main tabs on a phone: profile on the left, sync and watch on the right. */
export function MainBar() {
  return (
    <div className="mainbar">
      <ProfileLink />
      <div className="mainbar__right">
        <SyncAndWatch />
      </div>
    </div>
  );
}
