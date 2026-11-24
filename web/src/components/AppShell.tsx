import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Link, Outlet, useLocation } from 'react-router-dom';
import {
  ActivitiesIcon,
  BackIcon,
  CalendarIcon,
  ChevronDownIcon,
  HealthIcon,
  HomeIcon,
  ImportIcon,
  NextIcon,
  PerformanceIcon,
  PlanIcon,
  TargetIcon,
  TodayIcon,
  UserIcon,
  WatchIcon,
} from './icons';
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

// ---- desktop sidebar -------------------------------------------------------------------------------

interface SideLink {
  to: string;
  label: string;
}
interface SideGroup {
  key: string;
  label: string;
  icon: ReactNode;
  items: SideLink[];
}
type SideEntry = ({ kind: 'link'; icon: ReactNode } & SideLink) | ({ kind: 'group' } & SideGroup) | { kind: 'rule' };

/** The sidebar from 1024 px, as laid out in docs/design/web-layout/SPEC.md (only pages that exist). */
export const SIDEBAR: SideEntry[] = [
  { kind: 'link', to: '/', label: 'Home', icon: <HomeIcon /> },
  { kind: 'link', to: '/calendar', label: 'Calendar', icon: <CalendarIcon /> },
  { kind: 'rule' },
  {
    kind: 'group',
    key: 'activities',
    label: 'Activities',
    icon: <ActivitiesIcon />,
    items: [
      { to: '/activities', label: 'All Activities' },
      { to: '/strength', label: 'Strength' },
      { to: '/metrics/steps', label: 'Steps' },
      { to: '/metrics/intensity_minutes', label: 'Intensity Minutes' },
      { to: '/movement', label: 'Movement' },
    ],
  },
  {
    kind: 'group',
    key: 'health',
    label: 'Health Stats',
    icon: <HealthIcon />,
    items: [
      { to: '/sleep', label: 'Sleep' },
      { to: '/metrics/heart_rate', label: 'Heart Rate' },
      { to: '/metrics/stress', label: 'Stress' },
      { to: '/metrics/energy', label: 'Energy' },
      { to: '/metrics/respiration', label: 'Respiration' },
      { to: '/metrics/spo2', label: 'Pulse Ox' },
      { to: '/body', label: 'Weight & Body' },
      { to: '/day', label: 'Day' },
    ],
  },
  {
    kind: 'group',
    key: 'performance',
    label: 'Performance Stats',
    icon: <PerformanceIcon />,
    items: [
      { to: '/training/status', label: 'Training Status' },
      { to: '/hrv', label: 'HRV Status' },
      { to: '/training', label: 'Training' },
      { to: '/metrics/training_load', label: 'Training Load' },
      { to: '/metrics/vo2_max', label: 'VO2 Max' },
    ],
  },
  { kind: 'group', key: 'planning', label: 'Training & Planning', icon: <PlanIcon />, items: [{ to: '/workouts', label: 'Workouts' }] },
  { kind: 'rule' },
  { kind: 'link', to: '/devices', label: 'Watch', icon: <WatchIcon /> },
  { kind: 'link', to: '/profile/goals', label: 'Goals', icon: <TargetIcon /> },
  { kind: 'link', to: '/profile/import-garmin', label: 'Import', icon: <ImportIcon /> },
];

/** The admin area has no data: its sidebar holds the admin page and the profile. */
const ADMIN_SIDEBAR: SideEntry[] = [
  { kind: 'link', to: '/admin', label: 'Admin', icon: <HomeIcon /> },
  { kind: 'link', to: '/profile', label: 'Profile', icon: <UserIcon /> },
];

/**
 * The sidebar link a location belongs to (its `to`), or null when none does (the profile pages, which
 * the avatar in the header stands for). `/metrics/steps` is its own link; a day, a sleep night or
 * an activity belongs to the section that lists it.
 */
export function sidebarTargetFor(pathname: string, search: string): string | null {
  const starts = (p: string) => pathname === p || pathname.startsWith(`${p}/`);
  if (pathname === '/') return '/';
  if (starts('/calendar')) return '/calendar';
  if (starts('/activities') || starts('/strength')) {
    const strength = /(^|[?&])type=strength(&|$)/.test(search) || pathname.endsWith('/strength');
    return strength ? '/strength' : '/activities';
  }
  const metric = /^\/metrics\/([a-z0-9_]+)/.exec(pathname);
  if (metric) return `/metrics/${metric[1]}`;
  if (starts('/training/status')) return '/training/status';
  for (const p of ['/sleep', '/movement', '/body', '/day', '/hrv', '/training', '/workouts', '/devices', '/admin']) {
    if (starts(p)) return p;
  }
  if (starts('/profile/goals')) return '/profile/goals';
  if (starts('/profile/import-garmin')) return '/profile/import-garmin';
  if (starts('/profile')) return '/profile';
  return null;
}

const GROUP_KEY = (key: string) => `nimrag.nav.${key}`;
const COLLAPSED_KEY = 'nimrag.nav.collapsed';

function readFlag(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}
function writeFlag(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Storage blocked: the sidebar still works, it just does not remember.
  }
}

function useStoredOpen(entries: SideEntry[], target: string | null): [Record<string, boolean>, (key: string, open: boolean) => void] {
  const groupOf = (to: string | null) => entries.find((e): e is { kind: 'group' } & SideGroup => e.kind === 'group' && e.items.some((i) => i.to === to));
  const [open, setOpen] = useState<Record<string, boolean>>(() => {
    const here = groupOf(target)?.key;
    const state: Record<string, boolean> = {};
    for (const e of entries) {
      if (e.kind === 'group') state[e.key] = e.key === here || readFlag(GROUP_KEY(e.key)) === '1';
    }
    return state;
  });
  const toggle = (key: string, value: boolean) => {
    setOpen((o) => ({ ...o, [key]: value }));
    writeFlag(GROUP_KEY(key), value ? '1' : '0');
  };
  // Moving to a page in a closed group opens that group (state adjusted while rendering, not in an effect).
  const here = groupOf(target)?.key;
  const [seen, setSeen] = useState(here);
  if (seen !== here) {
    setSeen(here);
    if (here && !open[here]) setOpen({ ...open, [here]: true });
  }
  return [open, toggle];
}

interface SidebarProps {
  entries: SideEntry[];
  target: string | null;
  collapsed: boolean;
  onExpand: () => void;
}

/** The dark navigation column: links, groups that open and close, and an icon rail when collapsed. */
function Sidebar({ entries, target, collapsed, onExpand }: SidebarProps) {
  const [open, toggle] = useStoredOpen(entries, target);
  return (
    <nav aria-label="Main" className="sidebar__nav">
      {entries.map((e, i) => {
        if (e.kind === 'rule') return <div key={i} className="sidebar__rule" aria-hidden="true" />;
        if (e.kind === 'link') {
          return (
            <Link key={e.to} to={e.to} className="sidebar__item" aria-current={target === e.to ? 'page' : undefined} aria-label={e.label} title={collapsed ? e.label : undefined}>
              <span className="sidebar__icon">{e.icon}</span>
              <span className="sidebar__label">{e.label}</span>
            </Link>
          );
        }
        const isOpen = open[e.key] === true;
        const current = e.items.some((item) => item.to === target);
        const panel = `sidebar-${e.key}`;
        return (
          <div key={e.key} className="sidebar__group">
            <button
              type="button"
              className="sidebar__item sidebar__head"
              aria-expanded={isOpen && !collapsed}
              aria-controls={panel}
              aria-label={e.label}
              title={collapsed ? e.label : undefined}
              data-current={current ? 'true' : undefined}
              onClick={() => {
                // On the icon rail a group has no room to open: widen the sidebar and open it.
                if (collapsed) {
                  onExpand();
                  toggle(e.key, true);
                } else toggle(e.key, !isOpen);
              }}
            >
              <span className="sidebar__icon">{e.icon}</span>
              <span className="sidebar__label">{e.label}</span>
              <span className="sidebar__chevron" aria-hidden="true">
                <ChevronDownIcon size={16} />
              </span>
            </button>
            <ul id={panel} className="sidebar__sub" hidden={!isOpen || collapsed}>
              {e.items.map((item) => (
                <li key={item.to}>
                  <Link to={item.to} className="sidebar__subitem" aria-current={target === item.to ? 'page' : undefined}>
                    {item.label}
                  </Link>
                </li>
              ))}
            </ul>
          </div>
        );
      })}
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
   * The admin area: no data tabs, no sync or watch buttons. The sidebar holds the admin page and the
   * profile, the header the profile link; a phone gets a small top bar with the profile button.
   */
  area?: 'data' | 'admin';
  /** Render children instead of the router outlet (gallery, tests). */
  children?: ReactNode;
}

/**
 * Page frame. Phone: optional top bar, the page, bottom navigation.
 * From 1024 px: the sidebar (a 64 px icon rail when collapsed), a 60 px header with the sync and watch
 * buttons and the profile, and the page on a white sheet over a grey backdrop.
 */
export function AppShell({ tabs = false, area = 'data', children }: AppShellProps) {
  const admin = area === 'admin';
  const native = isNativeApp();
  const { pathname, search } = useLocation();
  const active = navKeyFor(pathname, search);
  const main = useRef<HTMLElement>(null);
  const first = useRef(true);
  const [collapsed, setCollapsed] = useState(() => readFlag(COLLAPSED_KEY) === '1');
  const setCollapsedStored = (value: boolean) => {
    setCollapsed(value);
    writeFlag(COLLAPSED_KEY, value ? '1' : '0');
  };

  // After a route change move focus to the page so keyboard and screen reader users start there.
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    main.current?.focus({ preventScroll: false });
  }, [pathname]);

  // The Android app has its own navigation: no web sidebar there.
  const sidebar = !native;
  return (
    <div className="shell" data-sidebar={sidebar ? (collapsed ? 'collapsed' : 'open') : 'none'}>
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      {sidebar ? (
        <aside className="sidebar" id="sidebar">
          <Link to={admin ? '/admin' : '/'} className="sidebar__brand" aria-label="Nimrag Connect">
            NIMRAG
          </Link>
          <Sidebar entries={admin ? ADMIN_SIDEBAR : SIDEBAR} target={sidebarTargetFor(pathname, search)} collapsed={collapsed} onExpand={() => setCollapsedStored(false)} />
        </aside>
      ) : null}
      <div className="shell__frame">
        <header className="shell__header">
          {sidebar ? (
            <button
              type="button"
              className="shell__collapse"
              aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
              aria-controls="sidebar"
              onClick={() => setCollapsedStored(!collapsed)}
            >
              {collapsed ? <NextIcon size={18} /> : <BackIcon size={18} />}
            </button>
          ) : null}
          <div className="shell__tools">
            {admin ? null : <SyncAndWatch />}
            <ProfileLink />
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
      </div>
      {/* In the Android app the native bottom bar replaces this one. */}
      {tabs && !admin && !native ? <BottomNav active={active} /> : null}
    </div>
  );
}
