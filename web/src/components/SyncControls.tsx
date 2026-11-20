import { useCallback, useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { getBridge, hasNativeSync, isNativeApp, openWatch, type LinkState } from '../native';
import { useNativeStatus } from '../lib/hooks';
import { RefreshIcon, WatchIcon } from './icons';
import { ProfileLink } from './ProfileLink';

const MIN_SPIN_MS = 700;
/** After a tap in the app, how long to wait for its sync to start before giving up (watch out of reach). */
const START_GRACE_MS = 15_000;

/**
 * Sync behaviour of the top bar.
 * In the Android app (`window.NimragApp.syncNow` exists) it asks the app to sync with the
 * watch, and spins for as long as the app reports `syncing` (also for syncs the app starts on
 * its own). In a normal browser it refetches every query from the server.
 */
export function useSync(): { sync: () => void; spinning: boolean; native: boolean } {
  const queryClient = useQueryClient();
  const { syncing } = useNativeStatus();
  const [spinning, setSpinning] = useState(false);
  // A tap in the app: spin until its sync has started and ended, or the start grace runs out.
  const [awaiting, setAwaiting] = useState(false);
  const started = useRef(false);
  const syncingNow = useRef(syncing);
  const grace = useRef<number | undefined>(undefined);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      window.clearTimeout(grace.current);
    };
  }, []);
  useEffect(() => {
    syncingNow.current = syncing;
    if (syncing) {
      started.current = true;
    } else if (started.current) {
      started.current = false;
      window.clearTimeout(grace.current);
      setAwaiting(false);
    }
  }, [syncing]);
  const native = hasNativeSync();

  const sync = useCallback(() => {
    setSpinning(true);
    const minimum = new Promise<void>((resolve) => window.setTimeout(resolve, MIN_SPIN_MS));
    const bridge = getBridge();
    const work = (async () => {
      if (typeof bridge?.syncNow === 'function') {
        // A tap during a running pull waits for that pull to end.
        started.current = syncingNow.current;
        setAwaiting(true);
        window.clearTimeout(grace.current);
        grace.current = window.setTimeout(() => {
          if (alive.current && !started.current) setAwaiting(false);
        }, START_GRACE_MS);
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

  return { sync, spinning: spinning || awaiting || syncing, native };
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

/**
 * Watch icon with a status dot; the state is also written in the aria-label.
 * In a browser it links to Devices; in the Android app it opens the app's own Watch screen.
 */
export function WatchLink({ link }: { link: LinkState }) {
  const text = `Watch ${LINK_TEXT[link]}`;
  const icon = (
    <>
      <WatchIcon />
      <span className="status-dot" data-state={link} aria-hidden="true" />
    </>
  );
  if (isNativeApp() && getBridge()?.openWatch) {
    return (
      <button type="button" className="icon-btn" aria-label={text} title={text} onClick={() => openWatch()}>
        {icon}
      </button>
    );
  }
  return (
    <Link to="/devices" className="icon-btn" aria-label={text} title={text}>
      {icon}
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
    // A banner landmark, so the buttons are not outside every landmark (axe: region).
    <header className="mainbar">
      <ProfileLink />
      <div className="mainbar__right">
        <SyncAndWatch />
      </div>
    </header>
  );
}
