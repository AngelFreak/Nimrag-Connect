import { expect, test } from '@playwright/test';

// Review screenshots for the design check: `npm run screenshots` writes web/screenshots/
// (gitignored). Not part of the smoke project.

const SIZES = [
  { name: '390', width: 390, height: 844 },
  { name: '1360', width: 1360, height: 1120 },
] as const;

const PAGES = [
  { name: 'gallery', path: '/__gallery' },
  { name: 'today', path: '/' },
  { name: 'login', path: '/login' },
] as const;

for (const size of SIZES) {
  for (const p of PAGES) {
    test(`${p.name} at ${size.name} px`, async ({ page }) => {
      await page.setViewportSize({ width: size.width, height: size.height });
      await page.goto(p.path);
      await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible();
      await page.evaluate(() => document.fonts.ready);
      if (p.name === 'gallery') await expect(page.locator('.uplot canvas').first()).toBeVisible();
      await page.screenshot({ path: `screenshots/${p.name}-${size.name}.png`, fullPage: p.name === 'gallery' });
    });
  }
}
