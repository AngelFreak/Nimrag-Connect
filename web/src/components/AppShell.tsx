import { useEffect, useRef, type ReactNode } from 'react';
import { Link, Outlet, useLocation } from 'react-router-dom';
import { ActivitiesIcon, HealthIcon, TodayIcon } from './icons';
import { isNativeApp } from '../native';
import { ProfileLink } from './ProfileLink';
import { MainBar, SyncAndWatch } from './SyncControls';

export type NavKey = 'today' | 'activities' | 'sleep' | 'health' | 'strength' | 'devices' | 'other';

/** Which top-level destination a location belongs to (drives aria-current). */
export function navKeyFor(pathname: string, search: string): NavKey {
  const strength = /(^|[?&])type=strength(&|$)/.test(search);
  if (pathname === '/' || pathname.startsWith('/day')) return 'today';
  if (pathname.startsWith('/activities')) return strength || pathname.endsWith('/strength') ? 'strength' : 'activities';
  if (pathname.startsWith('/workouts') || pathname.startsWith('/strength')) return 'strength';
  if (pathname.startsWith('/sleep')) return 'sleep';
  if (['/health', '/movement', '/training', '/body', '/calendar'].some((p) => pathname.startsWith(p))) return 'health';
  if (pathname.startsWith('/devices')) return 'devices';
  return 'other';
}

interface NavItem {
  key: NavKey;
  to: string;
  label: string;
  icon?: ReactNode;
}

/** Bottom navigation on a phone: exactly Today, Activities, Health (Strength is an Activities filter). */
const PHONE_NAV: NavItem[] = [
  { key: 'today', to: '/', label: 'Today', icon: <TodayIcon /> },
  { key: 'activities', to: '/activities', label: 'Activities', icon: <ActivitiesIcon /> },
  { key: 'health', to: '/health', label: 'Health', icon: <HealthIcon /> },
];

/** Top navigation on the desktop dashboard, as drawn in Web.dc.html. */
const DESKTOP_NAV: NavItem[] = [
  { key: 'today', to: '/', label: 'Overview' },
  { key: 'activities', to: '/activities', label: 'Activities' },
  { key: 'sleep', to: '/sleep', label: 'Sleep' },
  { key: 'health', to: '/health', label: 'Health' },
  { key: 'strength', to: '/strength', label: 'Strength' },
  { key: 'devices', to: '/devices', label: 'Watch' },
];

/** Phone bottom navigation. */
export function BottomNav({ active, label = 'Main' }: { active: NavKey; label?: string }) {
  const current = active === 'strength' ? 'activities' : active;
  return (
    <nav aria-label={label} className="bottom-nav">
      {PHONE_NAV.map((n) => (
        <Link key={n.key} to={n.to} aria-current={current === n.key ? 'page' : undefined}>
          {n.icon}
          {n.label}
        </Link>
      ))}
    </nav>
  );
}

/** Desktop header navigation. */
export function DesktopNav({ active, label = 'Main' }: { active: NavKey; label?: string }) {
  return (
    <nav aria-label={label} className="desk-nav">
      {DESKTOP_NAV.map((n) => (
        <Link key={n.key} to={n.to} aria-current={active === n.key ? 'page' : undefined}>
          {n.label}
        </Link>
      ))}
    </nav>
  );
}

interface AppShellProps {
  /**
   * Main tab pages (Today, Activities, Health, and the Strength list) show the top bar with the profile,
   * sync and watch buttons and the bottom navigation on a phone. Detail pages show neither
   * and use their own back button.
   */
  tabs?: boolean;
  /**
   * The admin area: no data tabs, no sync or watch buttons. The header holds the brand (a link to
   * /admin) and the profile link; a phone gets a small top bar with the profile button.
   */
  area?: 'data' | 'admin';
  /** Render children instead of the router outlet (gallery, tests). */
  children?: ReactNode;
}

/**
 * Page frame. Phone: optional top bar, the page, bottom navigation.
 * From 1024 px: the dashboard layout with the navigation in a header, content up to 1280 px.
 */
export function AppShell({ tabs = false, area = 'data', children }: AppShellProps) {
  const admin = area === 'admin';
  const { pathname, search } = useLocation();
  const active = navKeyFor(pathname, search);
  const main = useRef<HTMLElement>(null);
  const first = useRef(true);

  // After a route change move focus to the page so keyboard and screen reader users start there.
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    main.current?.focus({ preventScroll: false });
  }, [pathname]);

  return (
    <div className="shell">
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <header className="shell__header">
        <div className="shell__header-inner">
          <Link to={admin ? '/admin' : '/'} className="shell__brand">
            Nimrag Connect
          </Link>
          {admin ? <div className="shell__spacer" /> : <DesktopNav active={active} />}
          <div className="shell__tools">
            {admin ? null : <SyncAndWatch />}
            <ProfileLink />
          </div>
        </div>
      </header>
      <div className="shell__body">
        {tabs ? <MainBar /> : null}
        {admin ? (
          // Phone only (the header takes over from 1024 px); a banner landmark like the main bar.
          <header className="mainbar">
            <ProfileLink />
          </header>
        ) : null}
        <main id="main" ref={main} tabIndex={-1} style={{ outline: 'none' }}>
          {children ?? <Outlet />}
        </main>
      </div>
      {/* In the Android app the native bottom bar replaces this one. */}
      {tabs && !admin && !isNativeApp() ? <BottomNav active={active} /> : null}
    </div>
  );
}
