import { expect, test, type Page } from '@playwright/test';
import { expectAxeClean, settled, VIEWPORTS, watch } from './helpers';
import { SETUP_ORIGIN } from './real-server';
import { totpNow } from './totp';

// The first-run setup against a SECOND real server on an empty data dir (real.setup.ts starts it
// on SETUP_ORIGIN). The tests run in order: first the pages are checked at both widths, which
// creates nothing (the account only exists after the last step), and the final test completes the
// setup.
//
// Two-factor: the secret is a fresh one that the page itself shows, so its first code is for the
// current step and no replay bookkeeping is needed.

test.use({ baseURL: SETUP_ORIGIN, storageState: { cookies: [], origins: [] } });
test.describe.configure({ mode: 'serial' });

const USERNAME = 'boss';
const PASSWORD = 'first admin password';

async function fillAccount(page: Page) {
  await page.getByLabel('Admin username').fill(USERNAME);
  await page.getByLabel('Password', { exact: true }).fill(PASSWORD);
  await page.getByLabel('Password again').fill(PASSWORD);
  await page.getByRole('button', { name: 'Continue' }).click();
}

for (const vp of VIEWPORTS) {
  test(`the setup page, step 1, passes axe at ${vp.name}`, async ({ page }) => {
    await page.setViewportSize({ width: vp.width, height: vp.height });
    // Nobody is signed in: the one 401 is the app asking who the visitor is (GET /api/me) before it leads them on.
    const problems = watch(page, SETUP_ORIGIN, ['GET /api/me 401']);
    // A new server sends every visitor to the setup page.
    await page.goto('/');
    await expect(page).toHaveURL(/\/setup$/);
    await settled(page, 'Nimrag Connect');
    await expect(page.getByText(/only sets the server up/)).toBeVisible();
    await expect(page.getByRole('button', { name: 'Continue' })).toBeDisabled();
    await expectAxeClean(page);
    expect(problems, 'console errors and failed requests').toEqual([]);
  });

  test(`the setup page, step 2 (QR code and secret), passes axe at ${vp.name}`, async ({ page }) => {
    await page.setViewportSize({ width: vp.width, height: vp.height });
    const problems = watch(page, SETUP_ORIGIN);
    await page.goto('/setup');
    await settled(page);
    await fillAccount(page);
    const qr = page.getByRole('img', { name: /QR code/ });
    await expect(qr).toBeVisible();
    // Drawn in the page as SVG: nothing was fetched for it (watch() fails on another origin).
    expect(await qr.evaluate((el) => el.tagName.toLowerCase())).toBe('svg');
    await expect(page.getByLabel('Secret key')).toHaveValue(/^[A-Z2-7]{32}$/);
    const code = page.getByLabel('6-digit code');
    await expect(code).toHaveAttribute('autocomplete', 'one-time-code');
    await expect(code).toHaveAttribute('inputmode', 'numeric');
    await expectAxeClean(page);
    expect(problems, 'console errors and failed requests').toEqual([]);
  });
}

test('the setup creates the admin after a right code, signs them in and lands in /admin', async ({ page }) => {
  await page.goto('/setup');
  await settled(page);
  await fillAccount(page);
  const secret = await page.getByLabel('Secret key').inputValue();
  // Nothing exists yet: the server still waits for its admin.
  expect(await (await page.request.get('/api/setup')).json()).toEqual({ needed: true });

  await page.getByLabel('6-digit code').fill('000000');
  await page.getByRole('button', { name: 'Create the admin account' }).click();
  await expect(page.getByRole('alert')).toContainText('That code is not right');
  await expect(page.getByLabel('6-digit code')).toHaveValue('');

  await page.getByLabel('6-digit code').fill(totpNow(secret));
  await page.getByRole('button', { name: 'Create the admin account' }).click();
  await expect(page).toHaveURL(/\/admin$/);
  await settled(page, 'Admin');
  await expect(page.getByRole('list', { name: 'Users' }).getByRole('listitem').filter({ hasText: USERNAME })).toContainText('Two-factor on');
  await expect(page.getByRole('navigation', { name: 'Main' })).toHaveCount(0);
  expect(await (await page.request.get('/api/setup')).json()).toEqual({ needed: false });
  expect((await page.request.get('/api/today')).status()).toBe(403);

  // Setup is closed for good: the page leads to the login page now.
  await page.goto('/setup');
  await expect(page).toHaveURL(/\/login/);
});
