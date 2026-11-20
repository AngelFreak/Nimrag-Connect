import fs from 'node:fs';
import path from 'node:path';
import { test } from '@playwright/test';
import { settled, VIEWPORTS } from './helpers';
import { ORIGIN, webDir } from './real-server';

// Review screenshots of the Training status, HRV status and Sleep score pages at phone and desktop
// width, against the real server with the seeded data. Off by default; write them with
//   NIMRAG_SCREENSHOTS=1 npx playwright test --project=real real.screenshots
// into web/screenshots/ (gitignored).

test.skip(!process.env['NIMRAG_SCREENSHOTS'], 'set NIMRAG_SCREENSHOTS=1 to write the review screenshots');

const PAGES: [name: string, path: string][] = [
  ['training-status-current', '/training/status'],
  ['training-status-12w', '/training/status?range=12w'],
  ['hrv-7d', '/hrv'],
  ['hrv-4w', '/hrv?range=4w'],
  ['hrv-1d', '/hrv?range=1d'],
  ['sleep-score-7d', '/sleep?range=7d'],
  ['sleep-score-4w', '/sleep?range=4w'],
  ['sleep-score-1y', '/sleep?range=1y'],
  ['sleep-night-1d', '/sleep'],
];

const out = path.join(webDir, 'screenshots');

for (const vp of VIEWPORTS) {
  test.describe(vp.name, () => {
    test.use({ viewport: { width: vp.width, height: vp.height }, baseURL: ORIGIN });
    for (const [name, route] of PAGES) {
      test(name, async ({ page }) => {
        fs.mkdirSync(out, { recursive: true });
        await page.goto(route);
        await settled(page);
        await page.waitForTimeout(300);
        await page.screenshot({ path: path.join(out, `${name}-${vp.width}.png`), fullPage: true });
      });
    }
  });
}
