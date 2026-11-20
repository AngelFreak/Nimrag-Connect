import { execFileSync } from 'node:child_process';
import { expect, test, type Browser, type Page } from '@playwright/test';
import { expectAxeClean, settled, VIEWPORTS, watch } from './helpers';
import { adminSessionFile, ORIGIN, readServerState } from './real-server';
import { nextTotp } from './totp';

// The admin area against the real server, signed in as the seeded admin (the session saved by the
// setup project), and the whole life of a user the admin creates.
//
// Two-factor and replay protection: sessions are shared, so the admin logs in nowhere here. The
// users this file creates have secrets of their own (read from the page while enrolling); every
// code comes from `nextTotp`, which hands out strictly newer 30 s steps per secret and waits for
// the clock when the next free step is more than one ahead (totp.ts). That is why the user flow
// takes a minute or so.

test.use({ storageState: adminSessionFile });

const USER = 'e2e-anna';
const TOKEN = 'e2e pixel';
const FIRST_PASSWORD = 'anna first password 1';
const SECOND_PASSWORD = 'anna second password 2';
const THIRD_PASSWORD = 'anna third password 3';

test('an admin has no data tabs: data routes lead to /admin, and the data API answers 403 without a loop', async ({ page }) => {
  const problems = watch(page, ORIGIN);
  await page.goto('/');
  await expect(page).toHaveURL(/\/admin$/);
  await settled(page, 'Admin');
  await expect(page.getByRole('navigation', { name: 'Main' })).toHaveCount(0);
  for (const name of ['Activities', 'Strength', 'Health', 'Today', 'Overview']) await expect(page.getByRole('link', { name, exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Refresh' })).toHaveCount(0);
  for (const path of ['/activities', '/settings', '/sleep', '/day', '/does-not-exist']) {
    await page.goto(path);
    await expect(page).toHaveURL(/\/admin$/);
  }
  expect((await page.request.get('/api/today')).status()).toBe(403);
  expect((await page.request.get('/api/tokens')).status()).toBe(403);
  expect(problems, 'console errors and failed requests').toEqual([]);
});

test('the profile menu of the admin shows the name and role, a link to the admin area and Log out', async ({ page }) => {
  await page.goto('/admin');
  await settled(page, 'Admin');
  await page.getByRole('button', { name: 'Profile and account' }).first().click();
  const menu = page.getByRole('group', { name: 'Account' }).first();
  await expect(menu).toContainText('admin');
  await expect(menu).toContainText('Admin');
  await expect(menu.getByRole('link', { name: 'Admin area' })).toHaveAttribute('href', '/admin');
  await expect(menu.getByRole('link', { name: 'Settings' })).toHaveCount(0);
  await expect(menu.getByRole('button', { name: 'Log out' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
});

for (const vp of VIEWPORTS) {
  test(`the admin page lists the seeded accounts and passes axe at ${vp.name}`, async ({ page }) => {
    await page.setViewportSize({ width: vp.width, height: vp.height });
    const problems = watch(page, ORIGIN);
    await page.goto('/admin');
    await settled(page, 'Admin');
    const list = page.getByRole('list', { name: 'Users' });
    const admin = list.getByRole('listitem').filter({ hasText: 'admin' }).first();
    await expect(admin).toContainText('You');
    await expect(admin).toContainText('Two-factor on');
    // Your own password and two-factor are changed elsewhere: no buttons on your own row.
    await expect(admin.getByRole('button')).toHaveCount(0);
    const demo = list.getByRole('listitem').filter({ hasText: 'demo' });
    await expect(demo).toContainText('Two-factor on');
    await expect(demo).toContainText('files');
    await expect(demo.getByRole('button', { name: 'Reset password of demo' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Unassigned data' })).toBeVisible();
    await expectAxeClean(page);
    expect(problems, 'console errors and failed requests').toEqual([]);
  });

  test(`the create-user dialog passes axe at ${vp.name}`, async ({ page }) => {
    await page.setViewportSize({ width: vp.width, height: vp.height });
    await page.goto('/admin');
    await settled(page, 'Admin');
    await page.getByRole('button', { name: 'Create user' }).click();
    const dialog = page.getByRole('dialog', { name: 'Create a user' });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByLabel('Role')).toHaveValue('user');
    await expectAxeClean(page);
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);
  });
}

/** A browser context with nobody signed in. */
async function guest(browser: Browser) {
  const context = await browser.newContext({ baseURL: ORIGIN, storageState: { cookies: [], origins: [] }, serviceWorkers: 'block' });
  return { context, page: await context.newPage() };
}

async function logInAs(page: Page, username: string, password: string, code?: string) {
  await page.goto('/login');
  await settled(page);
  await page.getByLabel('Username').fill(username);
  await page.getByLabel('Password').fill(password);
  if (code) await page.getByLabel('6-digit code').fill(code);
  await page.getByRole('button', { name: 'Log in' }).click();
}

async function logOut(page: Page) {
  await page.getByRole('button', { name: 'Profile and account' }).first().click();
  await page.getByRole('group', { name: 'Account' }).first().getByRole('button', { name: 'Log out' }).click();
  await expect(page).toHaveURL(/\/login/);
}

test.describe('a user created by the admin', () => {
  test.describe.configure({ mode: 'serial', timeout: 180_000 });
  let temporary = '';
  let secret = '';
  let secondSecret = '';

  test('the admin creates the user; the temporary password is shown once, with a copy button', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await page.goto('/admin');
    await settled(page, 'Admin');
    await page.getByRole('button', { name: 'Create user' }).click();
    const dialog = page.getByRole('dialog', { name: 'Create a user' });
    await dialog.getByLabel('Username').fill(USER.toUpperCase());
    await dialog.getByRole('button', { name: 'Create user' }).click();

    const shown = page.getByRole('dialog', { name: `User created: ${USER}` });
    await expect(shown.getByRole('alert')).toContainText('shown only once');
    const field = shown.getByLabel(`Temporary password for ${USER}`);
    temporary = await field.inputValue();
    expect(temporary.length).toBeGreaterThanOrEqual(10);
    await shown.getByRole('button', { name: 'Copy password' }).click();
    await expect(shown.getByText('Copied to the clipboard.')).toBeVisible();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(temporary);
    await shown.getByRole('button', { name: 'I have copied it' }).click();

    // Gone for good: not in the page, and a reload does not bring it back.
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await expect(page.locator('body')).not.toContainText(temporary);
    const row = page.getByRole('list', { name: 'Users' }).getByRole('listitem').filter({ hasText: USER });
    await expect(row).toContainText('Two-factor not set up');
    await expect(row).toContainText('temporary password');
    await page.reload();
    await settled(page, 'Admin');
    await expect(page.locator('body')).not.toContainText(temporary);
  });

  test('an unassigned device token is handed to the user, and the card then says nothing is left', async ({ page }) => {
    // A token made on the command line without --user is unassigned until an admin assigns it.
    const { bin, dataDir } = readServerState();
    execFileSync(bin, ['token', 'create', '--name', TOKEN, '--data-dir', dataDir], { stdio: ['ignore', 'ignore', 'ignore'] });
    await page.goto('/admin');
    await settled(page, 'Admin');
    const card = page.getByRole('form', { name: 'Assign unassigned data' });
    await expect(card.getByText(TOKEN)).toBeVisible();
    await expect(card.getByRole('button', { name: 'Assign' })).toBeDisabled();
    await expectAxeClean(page);
    await page.setViewportSize({ width: 390, height: 844 });
    await expectAxeClean(page);

    await card.getByRole('checkbox', { name: new RegExp(TOKEN) }).check();
    // Only enabled users can receive data; the admin is not offered.
    await expect(card.getByLabel('Give to').locator('option', { hasText: 'admin' })).toHaveCount(0);
    await card.getByLabel('Give to').selectOption({ label: USER });
    await card.getByRole('button', { name: 'Assign' }).click();
    await expect(page.getByRole('status').filter({ hasText: `to ${USER}` })).toContainText('Gave 1 device token');
    await expect(page.getByText('Nothing is unassigned')).toBeVisible();
    const row = page.getByRole('list', { name: 'Users' }).getByRole('listitem').filter({ hasText: USER });
    await expect(row).toContainText('1 active token');
  });

  test('the user logs in with the temporary password, chooses a password and sets up two-factor', async ({ browser }) => {
    const { context, page } = await guest(browser);
    const problems = watch(page, ORIGIN);
    await logInAs(page, USER, temporary);

    // First login: no code is asked, two-factor is set up before anything else, and the
    // temporary password must be replaced at the same time.
    await expect(page.getByRole('img', { name: /QR code/ })).toBeVisible();
    await settled(page);
    secret = await page.getByLabel('Secret key').inputValue();
    expect(secret).toMatch(/^[A-Z2-7]{32}$/);
    await expectAxeClean(page);
    await page.getByLabel('New password', { exact: true }).fill(FIRST_PASSWORD);
    await page.getByLabel('New password again').fill(FIRST_PASSWORD);
    await page.getByLabel('6-digit code').fill(await nextTotp(secret));
    await page.getByRole('button', { name: 'Finish and log in' }).click();

    await expect(page).toHaveURL(/\/$/);
    await settled(page);
    await expect(page.getByRole('navigation', { name: 'Main' }).first()).toBeVisible();
    // The token the admin handed over is theirs now.
    const tokens = (await (await page.request.get('/api/tokens')).json()) as { tokens: { name: string }[] };
    expect(tokens.tokens.map((t) => t.name)).toEqual([TOKEN]);
    // A normal user has no admin area.
    expect((await page.request.get('/api/admin/users')).status()).toBe(403);
    await page.goto('/admin');
    await settled(page, 'Page not found');
    expect(problems, 'console errors and failed requests').toEqual([]);
    await context.close();
  });

  test('the user changes two-factor and password in Settings, then logs in with the new ones', async ({ browser }) => {
    const { context, page } = await guest(browser);
    await logInAs(page, USER, FIRST_PASSWORD, await nextTotp(secret));
    await expect(page).toHaveURL(/\/$/);
    await page.goto('/settings');
    await settled(page, 'Settings');

    const twoFactor = page.getByRole('heading', { name: 'Two-factor' }).locator('xpath=ancestor::section[1]');
    await twoFactor.getByRole('button', { name: 'Set up two-factor again' }).click();
    await twoFactor.getByLabel('Your password', { exact: true }).fill(FIRST_PASSWORD);
    await twoFactor.getByRole('button', { name: 'Continue' }).click();
    await expect(twoFactor.getByRole('img', { name: /QR code/ })).toBeVisible();
    secondSecret = await twoFactor.getByLabel('Secret key').inputValue();
    expect(secondSecret).not.toBe(secret);
    await twoFactor.getByLabel('6-digit code').fill(await nextTotp(secondSecret));
    await twoFactor.getByRole('button', { name: 'Confirm' }).click();
    await expect(twoFactor.getByRole('status')).toContainText('Two-factor is set up with the new authenticator');
    await expectAxeClean(page);

    const password = page.getByRole('heading', { name: 'Change password' }).locator('xpath=ancestor::section[1]');
    await password.getByLabel('Current password').fill(FIRST_PASSWORD);
    await password.getByLabel('New password', { exact: true }).fill(SECOND_PASSWORD);
    await password.getByLabel('New password again').fill(SECOND_PASSWORD);
    await password.getByRole('button', { name: 'Change password' }).click();
    await expect(password.getByRole('status')).toContainText('Password changed');
    // The fresh session works: still signed in.
    expect((await page.request.get('/api/me')).status()).toBe(200);
    await logOut(page);

    // The old authenticator and the old password no longer work; the new ones do.
    await logInAs(page, USER, SECOND_PASSWORD, await nextTotp(secondSecret));
    await expect(page).toHaveURL(/\/$/);
    await settled(page);
    await context.close();
  });

  test('after a password reset by the admin the user must choose a new password at the next login', async ({ page, browser }) => {
    await page.goto('/admin');
    await settled(page, 'Admin');
    await page.getByRole('button', { name: `Reset password of ${USER}` }).click();
    await page.getByRole('dialog', { name: `Reset the password of ${USER}?` }).getByRole('button', { name: 'Reset password' }).click();
    const shown = page.getByRole('dialog', { name: `New temporary password for ${USER}` });
    const temporaryAgain = await shown.getByLabel(`Temporary password for ${USER}`).inputValue();
    expect(temporaryAgain).not.toBe(temporary);
    await shown.getByRole('button', { name: 'I have copied it' }).click();

    const { context, page: user } = await guest(browser);
    // Two-factor is still set up, so the code is asked for; then the temporary password must be replaced.
    await logInAs(user, USER, temporaryAgain, await nextTotp(secondSecret));
    await expect(user.getByRole('button', { name: 'Save password and log in' })).toBeVisible();
    await expectAxeClean(user);
    await user.getByLabel('New password', { exact: true }).fill(THIRD_PASSWORD);
    await user.getByLabel('New password again').fill(THIRD_PASSWORD);
    await user.getByRole('button', { name: 'Save password and log in' }).click();
    await expect(user).toHaveURL(/\/$/);
    await settled(user);
    await context.close();
  });

  test('deleting the user takes two steps and hands their token back to Unassigned data', async ({ page }) => {
    await page.goto('/admin');
    await settled(page, 'Admin');
    await page.getByRole('button', { name: `Delete ${USER}` }).click();
    const first = page.getByRole('dialog', { name: `Delete ${USER}?` });
    await expect(first).toContainText('original files are never deleted');
    await first.getByRole('button', { name: 'Continue' }).click();
    const second = page.getByRole('dialog', { name: `Delete ${USER} and unassign their data?` });
    await expectAxeClean(page);
    await second.getByRole('button', { name: 'Delete and unassign' }).click();
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await expect(page.getByRole('list', { name: 'Users' }).getByRole('listitem').filter({ hasText: USER })).toHaveCount(0);
    // The device token is unassigned again.
    await expect(page.getByRole('form', { name: 'Assign unassigned data' }).getByText(TOKEN)).toBeVisible();
  });
});
