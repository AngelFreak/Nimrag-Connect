import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type BrowserContext, type Page } from '@playwright/test';
import { settled } from './helpers';
import { auditLayout, MIN_TARGET, type Finding } from './layout';
import { nextTotp } from './totp';
import { adminSessionFile, DEMO, fixtureFile, ORIGIN, sessionFile, stateDir, webDir, type Fixture } from './real-server';

// The layout audit: every route, dialog and state, as the owner sees them, must fit the screen.
//
// For each page at 320, 360, 390 and 412 px inside the Android app (window.NimragApp.native, so no
// web bottom bar), at 390 px in a phone browser, and at 1024 and 1360 px on a desktop (one page
// load per group, resized between the measurements), it fails on
//   * the page scrolling sideways (documentElement.scrollWidth > viewport),
//   * any element sticking out past the viewport edge (outside a region that is meant to scroll),
//   * a horizontal or vertical scroll container nobody marked `data-scroll`,
//   * content clipped by overflow: hidden,
//   * a button, link or field smaller than 44 x 44 px (links inside running text excepted).
// See layout.ts for the exact rules. The seeded user also has two signed-in phones (a normal name and
// a long one), because a table of tokens was what once pushed the phone list off the screen.
//
// Everything runs against the real server (see real.setup.ts) with sessions the setup project saved,
// so no login happens here. Findings are appended to web/.e2e/layout-audit.jsonl (one line per page
// and size); `node scripts/layout-audit-summary.mjs` prints counts per page. Set
// NIMRAG_AUDIT_SHOTS=<label> to also write full-page screenshots to web/screenshots/<label>/.

const SHOTS = process.env['NIMRAG_AUDIT_SHOTS'];
const report = path.join(stateDir, 'layout-audit.jsonl');

interface Size {
  name: string;
  width: number;
  height: number;
  /** Inside the Android app's web view. */
  app: boolean;
}
const SIZES: Size[] = [
  { name: 'app-320', width: 320, height: 640, app: true },
  { name: 'app-360', width: 360, height: 780, app: true },
  { name: 'app-390', width: 390, height: 844, app: true },
  { name: 'app-412', width: 412, height: 915, app: true },
  { name: 'web-390', width: 390, height: 844, app: false },
  { name: 'web-1024', width: 1024, height: 800, app: false },
  { name: 'web-1360', width: 1360, height: 1000, app: false },
];

/** The app (all four phone widths, window.NimragApp injected) and a plain browser (phone, narrow desktop, desktop). */
const MODES = [
  { name: 'in the app', app: true, sizes: SIZES.filter((s) => s.app) },
  { name: 'in a browser', app: false, sizes: SIZES.filter((s) => !s.app) },
];

interface Scenario {
  name: string;
  path: (f: Fixture) => string;
  /** Gets the page into the state to measure (open a dialog, mock an answer) after it settled. */
  open?: (page: Page) => Promise<void>;
  /** Called before navigation (mock the API). */
  mock?: (page: Page) => Promise<void>;
  /** Skip the "settled" wait: the state under test is a loading one. */
  loading?: boolean;
}

const json = (body: unknown, status = 200) => ({ status, contentType: 'application/json', body: JSON.stringify(body) });

/** Every data call fails: the page must show its error state. */
async function failData(page: Page) {
  await page.route(/\/api\/(?!me\b|setup\b|health\b)/, (route) => route.fulfill(json({ error: 'internal' }, 500)));
}
/** Every data call hangs: the page must show its loading state. */
async function hangData(page: Page) {
  await page.route(/\/api\/(?!me\b|setup\b|health\b)/, () => new Promise(() => undefined));
}

const click = (name: string | RegExp, exact = false) => async (page: Page) => {
  await page.getByRole('button', { name, exact }).first().click();
};

const DEMO_PAGES: Scenario[] = [
  { name: 'Today', path: () => '/' },
  { name: 'Today, Edit home', path: () => '/', open: click('Edit home') },
  { name: 'Activities', path: () => '/activities' },
  { name: 'Activities, strength filter', path: () => '/activities?type=strength' },
  { name: 'Strength list', path: () => '/strength' },
  { name: 'Activity detail', path: (f) => `/activities/${f.runId}` },
  { name: 'Strength session', path: (f) => `/activities/${f.strengthId}/strength` },
  { name: 'Sleep, latest night', path: () => '/sleep' },
  { name: 'Sleep, one night', path: (f) => `/sleep/${f.night}` },
  { name: 'Sleep score, 7 days', path: () => '/sleep?range=7d' },
  { name: 'Sleep score, 4 weeks', path: () => '/sleep?range=4w' },
  { name: 'Sleep score, 1 year', path: () => '/sleep?range=1y' },
  { name: 'Day', path: () => '/day' },
  { name: 'Day, one day', path: (f) => `/day/${f.night}` },
  { name: 'Health', path: () => '/health' },
  { name: 'Movement', path: () => '/movement' },
  { name: 'Training', path: () => '/training' },
  { name: 'Training status', path: () => '/training/status' },
  { name: 'Training status, 12 weeks', path: () => '/training/status?range=12w' },
  { name: 'HRV status, 7 days', path: () => '/hrv' },
  { name: 'HRV status, 4 weeks', path: () => '/hrv?range=4w' },
  { name: 'HRV status, one night', path: () => '/hrv?range=1d' },
  { name: 'Metric, heart_rate, 7 days', path: () => '/metrics/heart_rate?range=7d' },
  { name: 'Metric, stress, 7 days', path: () => '/metrics/stress?range=7d' },
  { name: 'Metric, energy, 7 days', path: () => '/metrics/energy?range=7d' },
  { name: 'Metric, steps, 7 days', path: () => '/metrics/steps?range=7d' },
  { name: 'Metric, intensity_minutes, 7 days', path: () => '/metrics/intensity_minutes?range=7d' },
  { name: 'Metric, respiration, 7 days', path: () => '/metrics/respiration?range=7d' },
  { name: 'Metric, spo2, 7 days', path: () => '/metrics/spo2?range=7d' },
  { name: 'Metric, steps, 1 year', path: () => '/metrics/steps?range=1y' },
  { name: 'Metric, steps, one day', path: () => '/metrics/steps?range=1d' },
  { name: 'Metric, training load, 12 weeks', path: () => '/metrics/training_load?range=12w' },
  { name: 'Metric, VO2 max, 6 months', path: () => '/metrics/vo2_max?range=6m' },
  { name: 'Body', path: () => '/body' },
  { name: 'Calendar', path: () => '/calendar' },
  { name: 'New workout', path: () => '/workouts/new' },
  { name: 'Watch', path: () => '/devices' },
  { name: 'Watch details', path: () => '/devices/details' },
  { name: 'Profile', path: () => '/profile' },
  { name: 'Profile, preferences section', path: () => '/profile?section=preferences' },
  { name: 'Profile, log out confirmation', path: () => '/profile', open: click('Log out') },
  { name: 'Password', path: () => '/profile/password' },
  { name: 'Two-step login', path: () => '/profile/two-step' },
  { name: 'Two-step login, password step', path: () => '/profile/two-step', open: click('Set up again') },
  { name: 'Signed-in phones', path: () => '/profile/phones' },
  { name: 'Signed-in phones, sign-out confirmation', path: () => '/profile/phones', open: click(/^Sign out /) },
  { name: 'Timezone', path: () => '/profile/timezone' },
  { name: 'Goals', path: () => '/profile/goals' },
  { name: 'Send activities to Garmin', path: () => '/profile/garmin' },
  { name: 'Page not found', path: () => '/does-not-exist' },
  // States: empty, loading, failed.
  {
    name: 'Activities, empty',
    path: () => '/activities',
    mock: async (page) => {
      await page.route(/\/api\/activities(\?|$)/, (route) => route.fulfill(json({ sample: false, items: [], next_before: null })));
    },
  },
  { name: 'Today, loading', path: () => '/', mock: hangData, loading: true },
  { name: 'Activities, loading', path: () => '/activities', mock: hangData, loading: true },
  { name: 'Profile, loading', path: () => '/profile', mock: hangData, loading: true },
  { name: 'Signed-in phones, loading', path: () => '/profile/phones', mock: hangData, loading: true },
  { name: 'Today, error', path: () => '/', mock: failData },
  { name: 'Activities, error', path: () => '/activities', mock: failData },
  { name: 'Health, error', path: () => '/health', mock: failData },
  { name: 'Profile, error', path: () => '/profile', mock: failData },
  { name: 'Signed-in phones, error', path: () => '/profile/phones', mock: failData },
  { name: 'Activity detail, error', path: (f) => `/activities/${f.runId}`, mock: failData },
  { name: 'Watch, error', path: () => '/devices', mock: failData },
];

const dialog = (open: string, name: RegExp | string) => async (page: Page) => {
  await page.getByRole('button', { name: open }).first().click();
  await expect(page.getByRole('dialog', { name })).toBeVisible();
};

const ADMIN_PAGES: Scenario[] = [
  { name: 'Admin', path: () => '/admin' },
  { name: 'Admin, create user dialog', path: () => '/admin', open: dialog('Create user', 'Create a user') },
  { name: 'Admin, reset password dialog', path: () => '/admin', open: dialog('Reset password of demo', /Reset the password/) },
  { name: 'Admin, delete user dialog', path: () => '/admin', open: dialog('Delete demo', /Delete demo/) },
  { name: 'Admin, error', path: () => '/admin', mock: failData },
];

const GUEST_PAGES: Scenario[] = [
  { name: 'Login', path: () => '/login' },
  {
    name: 'Setup',
    path: () => '/setup',
    mock: async (page) => {
      await page.route('**/api/setup', (route) => route.fulfill(json({ needed: true })));
    },
  },
];

const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

async function appContext(context: BrowserContext, app: boolean) {
  if (!app) return;
  await context.addInitScript(() => {
    // What the Android app injects (see src/native.ts).
    (window as unknown as { NimragApp: unknown }).NimragApp = {
      native: true,
      syncNow: () => undefined,
      openWatch: () => undefined,
      onRoute: () => undefined,
      status: () => ({ link: 'connected', lastSyncAt: null }),
    };
  });
}

/**
 * Loads the scenario once and measures it at each size by resizing the window (the layout reacts
 * to a resize exactly as it does to a fresh load, and a dialog stays open while it does).
 */
async function measure(page: Page, scenario: Scenario, sizes: Size[], fixture: Fixture, project: string) {
  await page.setViewportSize({ width: sizes[0]!.width, height: sizes[0]!.height });
  await scenario.mock?.(page);
  await page.goto(scenario.path(fixture));
  if (scenario.loading) await page.waitForTimeout(800);
  else await settled(page);
  if (scenario.open) {
    await scenario.open(page);
    await page.waitForTimeout(250);
  }
  const problems: string[] = [];
  for (const size of sizes) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(120);
    const findings: Finding[] = await page.evaluate(auditLayout, MIN_TARGET);
    // Deduplicate: the same element reported by several rules of one kind is one problem.
    const seen = new Set<string>();
    const unique = findings.filter((f) => {
      const key = `${f.kind}|${f.element}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
    fs.mkdirSync(stateDir, { recursive: true });
    fs.appendFileSync(report, JSON.stringify({ project, page: scenario.name, size: size.name, findings: unique }) + '\n');
    problems.push(...unique.map((f) => `${size.name}: ${f.kind}: ${f.element} (${f.detail})`));
    if (SHOTS && (size.name === 'app-412' || size.name === 'web-1360')) {
      const dir = path.join(webDir, 'screenshots', SHOTS);
      fs.mkdirSync(dir, { recursive: true });
      await page.screenshot({ path: path.join(dir, `${slug(scenario.name)}-${size.width}.png`), fullPage: !scenario.open });
    }
  }
  expect(problems, `layout problems on ${scenario.name}`).toEqual([]);
}

function suite(title: string, pages: Scenario[], storageState: string | { cookies: []; origins: [] }, extra?: () => void) {
  test.describe(title, () => {
    test.use({ storageState, baseURL: ORIGIN });
    let fixture: Fixture;
    test.beforeAll(() => {
      fixture = JSON.parse(fs.readFileSync(fixtureFile, 'utf8')) as Fixture;
    });
    extra?.();
    for (const mode of MODES) {
      for (const scenario of pages) {
        test(`${scenario.name} ${mode.name}`, async ({ page, context }, info) => {
          await appContext(context, mode.app);
          await measure(page, scenario, mode.sizes, fixture, info.project.name);
        });
      }
    }
  });
}

// The owner has a signed-in phone ("pixel8a") and, to stress the layout, one with a long unbroken name. Phones are
// made the way the app makes them: by logging in with POST /api/app/login (users cannot create tokens in the web UI).
const TOKENS = ['pixel8a', 'Pixel-9a-in-the-hallway-charging-station-upstairs-nightstand'];

suite('signed in as a user', DEMO_PAGES, sessionFile, () => {
  // Two workers run this block: the first to take the lock creates the tokens, the other waits.
  test.beforeAll(async ({ playwright }) => {
    const done = path.join(stateDir, 'layout-tokens.done');
    const lock = path.join(stateDir, 'layout-tokens.lock');
    fs.mkdirSync(stateDir, { recursive: true });
    let mine = false;
    try {
      fs.mkdirSync(lock);
      mine = true;
    } catch {
      // another worker is creating them
    }
    if (!mine) {
      const deadline = Date.now() + 30_000;
      while (!fs.existsSync(done) && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 100));
      return;
    }
    const api = await playwright.request.newContext({ baseURL: ORIGIN, storageState: sessionFile, extraHTTPHeaders: { 'X-Nimrag': '1' } });
    const list = (await (await api.get('/api/tokens')).json()) as { tokens: { name: string; active: boolean }[] };
    for (const name of TOKENS) {
      if (list.tokens.some((t) => t.name === name && t.active)) continue;
      // A phone has no cookie: sign in from a context of its own.
      const phone = await playwright.request.newContext({ baseURL: ORIGIN, extraHTTPHeaders: { 'X-Nimrag': '1' } });
      const made = await phone.post('/api/app/login', {
        data: { username: DEMO.username, password: DEMO.password, code: await nextTotp(DEMO.totp), device_name: name },
      });
      expect(made.status(), `signing in phone ${name}`).toBe(201);
      await phone.dispose();
    }
    await api.dispose();
    fs.writeFileSync(done, 'ok');
  });
});
suite('signed in as the admin', ADMIN_PAGES, adminSessionFile);
suite('signed out', GUEST_PAGES, { cookies: [], origins: [] });
