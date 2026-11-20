// Shared by the real-server specs.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page } from '@playwright/test';

/** WCAG 2 A and AA (with the 2.1 additions), plus axe's best-practice rules (landmarks, heading order). */
export const AXE_TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'];

export const VIEWPORTS = [
  { name: 'phone 390', width: 390, height: 844 },
  { name: 'desktop 1360', width: 1360, height: 1000 },
];

/**
 * Collects everything that should never happen on a healthy page: page errors, console errors,
 * failed or error-status requests, and any request to another origin (the QR secret and the
 * fonts must stay on the server). `allow` names expected error responses ("POST /api/login 401").
 */
export function watch(page: Page, origin: string, allow: string[] = []) {
  const problems: string[] = [];
  page.on('pageerror', (e) => problems.push(`page error: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() !== 'error') return;
    // A server without a tile file answers the map probe with 404: expected, the page shows a placeholder.
    if (m.location().url.includes('/tiles/')) return;
    // The browser logs every error status as "Failed to load resource"; the response listener names it.
    if (allow.length > 0 && m.text().startsWith('Failed to load resource')) return;
    problems.push(`console error: ${m.text()}`);
  });
  page.on('requestfailed', (r) => {
    // A query cancelled by leaving the page is not a failure.
    if (r.failure()?.errorText === 'net::ERR_ABORTED') return;
    problems.push(`request failed: ${r.method()} ${r.url()} (${r.failure()?.errorText})`);
  });
  page.on('response', (r) => {
    const url = new URL(r.url());
    if (r.status() < 400 || url.pathname.startsWith('/tiles/')) return;
    if (allow.includes(`${r.request().method()} ${url.pathname} ${r.status()}`)) return;
    problems.push(`HTTP ${r.status()}: ${r.request().method()} ${r.url()}`);
  });
  page.on('request', (r) => {
    if (r.url().startsWith('data:') || r.url().startsWith('blob:')) return;
    if (new URL(r.url()).origin !== origin) problems.push(`third-party request: ${r.url()}`);
  });
  return problems;
}

/** The page has its heading, nothing is loading any more, and charts have drawn. */
export async function settled(page: Page, heading?: string) {
  const h1 = page.getByRole('heading', { level: 1 }).first();
  await expect(h1).toBeVisible();
  if (heading) await expect(h1).toContainText(heading);
  await page.waitForLoadState('networkidle');
  await expect(page.locator('[aria-busy="true"]')).toHaveCount(0);
  await expect(page.locator('.skeleton')).toHaveCount(0);
  await page.evaluate(() => document.fonts.ready);
}

/** axe (WCAG 2 A and AA, best practice) finds nothing on the page as it is now. */
export async function expectAxeClean(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(AXE_TAGS).analyze();
  const violations = results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).slice(0, 4).join(' | ')}`);
  expect(violations, 'axe violations').toEqual([]);
}
