import fs from 'node:fs';
import { expect, test } from '@playwright/test';
import { chartsInPage, pageFactsInPage, runAxe, tabThrough } from './access';
import { settled } from './helpers';
import { fixtureFile, ORIGIN, sessionFile, type Fixture } from './real-server';

// Companion of real.access.spec.ts: the accessibility checks themselves (they must catch what they
// are written for), and the interactions a page load does not show: a dialog, the calendar's
// arrow keys, and the map when a tile file exists.

const SIZES = [
  { name: 'phone 390x844', width: 390, height: 844 },
  { name: 'desktop 1360x900', width: 1360, height: 900 },
] as const;

test.describe('the checks catch what they are for', () => {
  test.use({ storageState: { cookies: [], origins: [] } });

  test('a page built wrong is reported by axe, the keyboard walk and the chart check', async ({ page }) => {
    await page.setContent(`<!doctype html><html><head><style>
        button.plain:focus { outline: none; }
      </style></head><body>
        <h1>One</h1><h1>Two</h1>
        <button class="plain">Looks fine, shows no focus</button>
        <div style="cursor: pointer" onclick="void 0">Clickable div</div>
        <button tabindex="-1">Out of the Tab order</button>
        <button id="trap">Keeps the focus</button>
        <svg width="200" height="80"><rect width="200" height="80" fill="#888"/></svg>
        <script>document.getElementById('trap').addEventListener('keydown', (e) => { if (e.key === 'Tab') e.preventDefault(); });</script>
      </body></html>`);
    const facts = await page.evaluate(pageFactsInPage);
    expect(facts.h1).toHaveLength(2);
    expect(facts.title).toBe('');
    const keyboard = (await tabThrough(page)).problems.join('\n');
    expect(keyboard).toContain('no visible focus indicator');
    expect(keyboard).toContain('Clickable div');
    expect(keyboard).toContain('Out of the Tab order');
    expect(keyboard).toContain('focus trap');
    expect((await page.evaluate(chartsInPage)).map((c) => c.problem)).toEqual(['no accessible name (aria-label)']);
    expect((await runAxe(page)).map((v) => v.rule)).toEqual(expect.arrayContaining(['document-title', 'html-has-lang']));
  });

  test('a chart whose name says nothing about its values is reported, one with a summary is not', async ({ page }) => {
    await page.setContent(`<!doctype html><html lang="en"><head><title>t</title></head><body><main>
        <section><svg role="img" aria-label="Heart rate" width="200" height="80"><rect width="200" height="80"/></svg></section>
        <section><svg role="img" aria-label="Heart rate over the day, between 52 and 148 beats per minute" width="200" height="80"><rect width="200" height="80"/></svg></section>
      </main></body></html>`);
    expect((await page.evaluate(chartsInPage)).map((c) => `${c.chart}: ${c.problem}`)).toEqual(['svg "Heart rate": has a name but no summary of what it shows']);
  });
});

test.describe('as the user: interactions', () => {
  test.use({ storageState: sessionFile, baseURL: ORIGIN });

  for (const size of SIZES) {
    test(`the log-out dialog traps Tab, closes with Escape and gives the focus back, and passes axe at ${size.name}`, async ({ page }) => {
      await page.setViewportSize({ width: size.width, height: size.height });
      await page.goto('/profile');
      await settled(page, 'Profile');
      const opener = page.getByRole('button', { name: 'Log out' });
      await opener.focus();
      await page.keyboard.press('Enter');
      const dialog = page.getByRole('dialog', { name: 'Log out?' });
      await expect(dialog).toBeVisible();
      expect(await runAxe(page)).toEqual([]);
      // Tab around more times than the dialog has controls: the focus never leaves it.
      for (let i = 0; i < 8; i++) {
        await page.keyboard.press('Tab');
        expect(await dialog.evaluate((d) => d.contains(document.activeElement)), `after Tab ${i + 1}`).toBe(true);
      }
      for (let i = 0; i < 4; i++) {
        await page.keyboard.press('Shift+Tab');
        expect(await dialog.evaluate((d) => d.contains(document.activeElement)), `after Shift+Tab ${i + 1}`).toBe(true);
      }
      await page.keyboard.press('Escape');
      await expect(dialog).toHaveCount(0);
      await expect(opener).toBeFocused();
    });
  }

  test('the calendar is one Tab stop, and the arrow keys move between its days', async ({ page }) => {
    await page.goto('/calendar');
    await settled(page);
    const stops = await page.locator('.cal-grid a[data-date]').evaluateAll((els) => els.filter((e) => (e as HTMLElement).tabIndex >= 0).length);
    expect(stops).toBe(1);
    await page.locator('.cal-grid a[tabindex="0"]').focus();
    const before = await page.evaluate(() => document.activeElement?.getAttribute('data-date'));
    await page.keyboard.press('ArrowLeft');
    const after = await page.evaluate(() => document.activeElement?.getAttribute('data-date'));
    expect(after).not.toBeNull();
    expect(after).not.toBe(before);
    await page.keyboard.press('ArrowRight');
    expect(await page.evaluate(() => document.activeElement?.getAttribute('data-date'))).toBe(before);
  });

  test('the route map with a tile file (MapLibre) passes axe and its controls can be reached with the keyboard', async ({ page }) => {
    // The probe only needs a 206 that is not HTML; MapLibre then draws its canvas and controls and
    // complains about the file's contents, which is not what is tested here.
    await page.route('**/tiles/map.pmtiles', (route) => route.fulfill({ status: 206, contentType: 'application/octet-stream', body: Buffer.alloc(16) }));
    const fixture = JSON.parse(fs.readFileSync(fixtureFile, 'utf8')) as Fixture;
    await page.goto(`/activities/${fixture.runId}`);
    await settled(page);
    const map = page.locator('.map__canvas');
    await expect(map).toBeVisible();
    await expect(map.locator('canvas.maplibregl-canvas')).toBeVisible({ timeout: 20_000 });
    expect(await runAxe(page)).toEqual([]);
    expect((await tabThrough(page)).problems).toEqual([]);
    expect(await page.evaluate(chartsInPage)).toEqual([]);
  });
});
