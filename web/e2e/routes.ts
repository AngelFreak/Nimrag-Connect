// Every route of src/App.tsx with the addresses a person really opens (tabs, ranges, a day), for the
// accessibility spec. The spec also checks that no route in App.tsx is missing here.

import { METRICS } from '../src/pages/metrics/period';
import type { Fixture } from './real-server';

/** Who looks at the page: the seeded user, the seeded admin, nobody (the main server), nobody on the empty server. */
export type Who = 'user' | 'admin' | 'guest' | 'setup';

export interface RouteCase {
  name: string;
  who: Who;
  /** The page shows a loading state on purpose (the component gallery): do not wait for it to end. */
  demo?: boolean;
  path: (f: Fixture) => string;
}

const user = (name: string, path: (f: Fixture) => string): RouteCase => ({ name, who: 'user', path });

const metrics: RouteCase[] = Object.values(METRICS).flatMap((def) =>
  def.ranges.map((range) => user(`Metric ${def.key}, ${range}${range === def.defaultRange ? ' (default tab)' : ''}`, () => `/metrics/${def.key}?range=${range}`)),
);

export const USER_ROUTES: RouteCase[] = [
  user('Today', () => '/'),
  user('Activities', () => '/activities'),
  user('Activities, strength filter', () => '/activities?type=strength'),
  user('Strength list', () => '/strength'),
  user('Activity detail, run with a map', (f) => `/activities/${f.runId}`),
  user('Activity detail, walk with a map', (f) => `/activities/${f.walkId}`),
  user('Activity detail, strength without a map', (f) => `/activities/${f.strengthId}`),
  user('Strength session', (f) => `/activities/${f.strengthId}/strength`),
  user('Sleep, latest night', () => '/sleep'),
  user('Sleep, one night', (f) => `/sleep/${f.night}`),
  user('Sleep score, 7 days', () => '/sleep?range=7d'),
  user('Sleep score, 4 weeks', () => '/sleep?range=4w'),
  user('Sleep score, 1 year', () => '/sleep?range=1y'),
  user('Sleep score, an earlier week', (f) => `/sleep?range=7d&end=${f.night}`),
  user('Day, today', () => '/day'),
  user('Day, one day', (f) => `/day/${f.night}`),
  user('Health', () => '/health'),
  user('Movement, today', () => '/movement'),
  user('Movement, one day', (f) => `/movement/${f.night}`),
  user('Training', () => '/training'),
  user('Training status, current', () => '/training/status'),
  user('Training status, 4 weeks', () => '/training/status?range=4w'),
  user('Training status, 12 weeks', () => '/training/status?range=12w'),
  user('Training status, 6 months', () => '/training/status?range=6m'),
  user('HRV status, 7 days', () => '/hrv'),
  user('HRV status, 4 weeks', () => '/hrv?range=4w'),
  user('HRV status, one night', () => '/hrv?range=1d'),
  ...metrics,
  user('Metric steps, one day with an end date', (f) => `/metrics/steps?range=1d&end=${f.night}`),
  user('Body', () => '/body'),
  user('Calendar, this month', () => '/calendar'),
  user('Calendar, one month', (f) => `/calendar/${f.month}`),
  user('Workouts', () => '/workouts'),
  user('New workout', () => '/workouts/new'),
  user('Workout, one saved', (f) => `/workouts/${f.workoutId}`),
  user('Watch', () => '/devices'),
  user('Watch details', () => '/devices/details'),
  user('Profile', () => '/profile'),
  user('Profile, preferences section', () => '/profile?section=preferences'),
  user('Profile, about section', () => '/profile?section=about'),
  user('Password', () => '/profile/password'),
  user('Two-step login', () => '/profile/two-step'),
  user('Signed-in phones', () => '/profile/phones'),
  user('Timezone', () => '/profile/timezone'),
  user('Goals', () => '/profile/goals'),
  user('Send activities to Garmin', () => '/profile/garmin'),
  { ...user('Component gallery', () => '/__gallery'), demo: true },
  user('Old settings address', () => '/settings'),
  user('Page not found', () => '/does-not-exist'),
];

export const ADMIN_ROUTES: RouteCase[] = [
  { name: 'Admin', who: 'admin', path: () => '/admin' },
  { name: 'Admin, profile', who: 'admin', path: () => '/profile' },
  { name: 'Admin, password', who: 'admin', path: () => '/profile/password' },
  { name: 'Admin, two-step login', who: 'admin', path: () => '/profile/two-step' },
];

export const GUEST_ROUTES: RouteCase[] = [
  { name: 'Login', who: 'guest', path: () => '/login' },
  { name: 'Setup, first step', who: 'setup', path: () => '/setup' },
];

export const ALL_ROUTES: RouteCase[] = [...USER_ROUTES, ...ADMIN_ROUTES, ...GUEST_ROUTES];

/** The `path` of every `<Route>` in App.tsx (`*` is the not-found page, listed above as a path that does not exist). */
export function routePatterns(appSource: string): string[] {
  return [...appSource.matchAll(/<Route[^>]*\spath="([^"]+)"/g)].map((m) => m[1]!).filter((p) => p !== '*');
}

/** `sleep/:date?` matches `/sleep` and `/sleep/2026-11-20`; a nested route (`password`) matches as the end of a path. */
export function patternMatches(pattern: string, pathname: string): boolean {
  const body = pattern
    .split('/')
    .filter(Boolean)
    .map((seg) => (seg.startsWith(':') ? (seg.endsWith('?') ? '(?:/[^/]+)?' : '/[^/]+') : `/${seg.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`))
    .join('');
  return new RegExp(`${body}$`).test(pathname);
}
