import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { chartsInPage, pageFactsInPage, runAxe, tabThrough } from './access';
import { settled, watch } from './helpers';
import { auditLayout, type Finding } from './layout';
import { adminSessionFile, fixtureFile, ORIGIN, sessionFile, SETUP_ORIGIN, webDir, type Fixture } from './real-server';
import { ADMIN_ROUTES, ALL_ROUTES, GUEST_ROUTES, patternMatches, routePatterns, USER_ROUTES, type RouteCase } from './routes';

// The accessibility check of Gate 3 (stages 3c to 3m): every route of src/App.tsx, as the person who
// can open it, at a phone size and a desktop size, in a light and a dark colour scheme.
//
// For each page and size:
//   * axe with the WCAG 2.0 and 2.1 A and AA rules plus its best-practice rules, once per colour
//     scheme (the scheme is switched on the loaded page with `emulateMedia`)
//   * exactly one visible h1, a non-empty `<title>` that names the page, a language, one `main`
//   * the keyboard: Tab from the top reaches every control, never gets stuck, every focused element
//     shows a focus indicator, nothing reacts to the mouse only (access.ts, `tabThrough`)
//   * every chart has a name and a summary in text (access.ts, `chartsInPage`)
// And at a phone size at 200 % browser zoom (a 195 px wide window: 390 / 2) no page scrolls sideways
// and no text is cut off.
//
// The app has one theme, dark (`color-scheme: dark` in styles/tokens.css, no `prefers-color-scheme`
// rule): a visitor with a light system gets the same pages, and the test proves that by running axe
// under both settings.

const SIZES = [
  { name: 'phone 390x844', width: 390, height: 844 },
  { name: 'desktop 1360x900', width: 1360, height: 900 },
] as const;
const SCHEMES = ['light', 'dark'] as const;

/** 390 px at 200 % zoom. */
const ZOOMED = { width: 195, height: 422 };

interface Group {
  title: string;
  routes: RouteCase[];
  storageState: string | { cookies: []; origins: [] };
  origin: string;
  /** Error responses the page is expected to get ("GET /api/me 401"). */
  allow: string[];
}
const GROUPS: Group[] = [
  { title: 'as the user', routes: USER_ROUTES, storageState: sessionFile, origin: ORIGIN, allow: [] },
  { title: 'as the admin', routes: ADMIN_ROUTES, storageState: adminSessionFile, origin: ORIGIN, allow: [] },
  // Nobody is signed in: the one 401 is the app asking who the visitor is before it shows the page.
  { title: 'signed out', routes: GUEST_ROUTES.filter((r) => r.who === 'guest'), storageState: { cookies: [], origins: [] }, origin: ORIGIN, allow: ['GET /api/me 401'] },
  { title: 'first run', routes: GUEST_ROUTES.filter((r) => r.who === 'setup'), storageState: { cookies: [], origins: [] }, origin: SETUP_ORIGIN, allow: ['GET /api/me 401'] },
];

let fixture: Fixture;
test.beforeAll(() => {
  fixture = JSON.parse(fs.readFileSync(fixtureFile, 'utf8')) as Fixture;
});

/** A frame after the scheme changed: styles recalculated, transitions started. */
async function afterPaint(page: Page) {
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await page.waitForTimeout(60);
}

test('every route of App.tsx is in the list this spec visits', () => {
  const source = fs.readFileSync(path.join(webDir, 'src', 'App.tsx'), 'utf8');
  const visited = ALL_ROUTES.map((r) => r.path(fixture).split('?')[0]!);
  const missing = routePatterns(source).filter((pattern) => !visited.some((p) => patternMatches(pattern, p)));
  expect(missing, 'routes in App.tsx that no case here opens').toEqual([]);
});

for (const group of GROUPS) {
  test.describe(group.title, () => {
    test.use({ storageState: group.storageState, baseURL: group.origin });

    for (const size of SIZES) {
      test.describe(size.name, () => {
        test.use({ viewport: { width: size.width, height: size.height } });

        for (const route of group.routes) {
          test(`${route.name}: axe in light and dark, h1 and title, keyboard, charts`, async ({ page }) => {
            const problems = watch(page, group.origin, group.allow);
            const issues: string[] = [];
            await page.emulateMedia({ colorScheme: 'light' });
            await page.goto(route.path(fixture));
            if (route.demo) await page.getByRole('heading', { level: 1 }).first().waitFor();
            else await settled(page);
            // The error screens are failures here: the seeded server answers every call.
            if (!route.demo) await expect(page.getByRole('alert')).toHaveCount(0);

            // Document: one h1, a title that says where we are, a language, one main region.
            const facts = await page.evaluate(pageFactsInPage);
            if (facts.h1.length !== 1) issues.push(`h1: ${facts.h1.length} visible level-1 headings (${facts.h1.map((h) => `"${h}"`).join(', ')})`);
            if (!facts.title.trim()) issues.push('title: the page has no <title>');
            else if (route.who !== 'setup' && route.who !== 'guest' && facts.title.trim() === 'Nimrag Connect') issues.push(`title: "${facts.title}" is the same on every page (use usePageTitle)`);
            if (!facts.lang) issues.push('lang: <html> has no lang');
            if (facts.mains !== 1) issues.push(`landmarks: ${facts.mains} main regions`);

            // Colours: both schemes, same page.
            for (const scheme of SCHEMES) {
              await page.emulateMedia({ colorScheme: scheme });
              await afterPaint(page);
              for (const v of await runAxe(page)) issues.push(`axe [${scheme}] ${v.rule} (${v.impact}): ${v.targets.join(' | ')}`);
            }

            for (const c of await page.evaluate(chartsInPage)) issues.push(`chart ${c.chart}: ${c.problem}`);
            const keyboard = await tabThrough(page);
            for (const p of keyboard.problems) issues.push(`keyboard: ${p}`);

            expect(issues, `accessibility problems on ${route.name} at ${size.name}`).toEqual([]);
            expect(problems, 'console errors and failed requests').toEqual([]);
          });
        }
      });
    }

    test.describe('phone at 200 % zoom', () => {
      test.use({ viewport: ZOOMED });

      for (const route of group.routes) {
        test(`${route.name}: no sideways scrolling and no cut-off text`, async ({ page }) => {
          await page.goto(route.path(fixture));
          if (route.demo) await page.getByRole('heading', { level: 1 }).first().waitFor();
          else await settled(page);
          await page.evaluate(() => document.fonts.ready);
          await page.waitForTimeout(150);
          const findings: Finding[] = await page.evaluate(auditLayout, 44);
          // Small targets are the layout audit's job (and are measured in CSS px, which do not shrink with zoom).
          const wanted = findings.filter((f) => f.kind !== 'small-target');
          const seen = new Set<string>();
          const unique = wanted.filter((f) => !seen.has(`${f.kind}|${f.element}`) && seen.add(`${f.kind}|${f.element}`));
          expect(
            unique.map((f) => `${f.kind}: ${f.element} (${f.detail})`),
            `layout at ${ZOOMED.width} px (390 px at 200 % zoom) on ${route.name}`,
          ).toEqual([]);
        });
      }
    });
  });
}
