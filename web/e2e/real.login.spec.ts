import { expect, test } from '@playwright/test';
import { expectAxeClean, settled, VIEWPORTS, watch } from './helpers';
import { ADMIN, DEMO, ORIGIN } from './real-server';
import { nextTotp } from './totp';

// The login page and the two seeded personas signing in through it, against the real server.
// Nobody is signed in at the start of these tests (no storage state).
//
// Two-factor and replay protection: the setup project already logged `admin` and `demo` in once
// (step S). The one extra login each of these tests does takes its code from `nextTotp`, which
// returns step S+1 (inside the server's +/-1 skew) for the first call and waits for the clock
// for any further one. Do not add a third login of the same persona without reading totp.ts.

test.use({ storageState: { cookies: [], origins: [] } });

for (const vp of VIEWPORTS) {
  test(`login page passes axe at ${vp.name}, with no failing request and nothing from another origin`, async ({ page }) => {
    await page.setViewportSize({ width: vp.width, height: vp.height });
    const problems = watch(page, ORIGIN);
    await page.goto('/login');
    await settled(page, 'Nimrag Connect');
    await expect(page.getByLabel('Username')).toBeVisible();
    await expect(page.getByLabel('Password')).toBeVisible();
    const code = page.getByLabel('6-digit code');
    await expect(code).toHaveAttribute('autocomplete', 'one-time-code');
    await expect(code).toHaveAttribute('inputmode', 'numeric');
    await expectAxeClean(page);
    expect(problems, 'console errors and failed requests').toEqual([]);
  });
}

// The server allows four wrong attempts per address before it slows down, and every test here
// comes from 127.0.0.1: keep the number of deliberately wrong logins in the suite small.
test('a wrong password and a wrong code give the same generic message', async ({ page }) => {
  await page.goto('/login');
  await settled(page);
  for (const [password, code] of [
    ['not the password', ''],
    [DEMO.password, '000000'],
  ] as const) {
    await page.getByLabel('Username').fill(DEMO.username);
    await page.getByLabel('Password').fill(password);
    await page.getByLabel('6-digit code').fill(code);
    await page.getByRole('button', { name: 'Log in' }).click();
    await expect(page.getByRole('alert')).toHaveText('Wrong username, password or code.');
    await expect(page).toHaveURL(/\/login/);
  }
});

test('the user demo logs in with username, password and code and sees Today', async ({ page }) => {
  const problems = watch(page, ORIGIN);
  await page.goto('/login');
  await settled(page);
  await page.getByLabel('Username').fill(DEMO.username);
  await page.getByLabel('Password').fill(DEMO.password);
  await page.getByLabel('6-digit code').fill(await nextTotp(DEMO.totp));
  await page.getByRole('button', { name: 'Log in' }).click();
  await expect(page).toHaveURL(/\/$/);
  await settled(page);
  await expect(page.getByRole('navigation', { name: 'Main' }).first()).toBeVisible();
  await expect(page.getByRole('link', { name: 'Today' }).first()).toBeVisible();
  await page.getByRole('button', { name: 'Profile and account' }).first().click();
  await expect(page.getByRole('group', { name: 'Account' }).first()).toContainText('demo');
  // The page loaded real data for the signed-in user.
  expect((await page.request.get('/api/me')).status()).toBe(200);
  expect(problems, 'console errors and failed requests').toEqual([]);
});

test('the admin logs in with username, password and code and lands in the admin area, without data tabs', async ({ page }) => {
  await page.goto('/login?next=%2Factivities');
  await settled(page);
  await page.getByLabel('Username').fill(ADMIN.username);
  await page.getByLabel('Password').fill(ADMIN.password);
  await page.getByLabel('6-digit code').fill(await nextTotp(ADMIN.totp));
  await page.getByRole('button', { name: 'Log in' }).click();
  // An admin has no data: the page they were heading to is replaced by the admin area.
  await expect(page).toHaveURL(/\/admin$/);
  await settled(page, 'Admin');
  await expect(page.getByRole('navigation', { name: 'Main' })).toHaveCount(0);
  await expect(page.getByRole('link', { name: 'Activities' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Profile and account' }).first().click();
  const menu = page.getByRole('group', { name: 'Account' }).first();
  await expect(menu).toContainText('admin');
  await expect(menu.getByRole('link', { name: 'Admin area' })).toBeVisible();
  await menu.getByRole('button', { name: 'Log out' }).click();
  await expect(page).toHaveURL(/\/login/);
  expect((await page.request.get('/api/admin/users')).status()).toBe(401);
});
