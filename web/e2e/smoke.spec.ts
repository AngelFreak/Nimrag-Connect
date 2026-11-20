import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

// Smoke tests against `vite preview` of the production build: the gallery and the login page,
// with the API mocked by the tests. Every route against the real server is in real.spec.ts.

/** Fails the test on page errors, console errors, and any request to another origin. */
function watch(page: Page, baseURL: string) {
  const problems: string[] = [];
  const origin = new URL(baseURL).origin;
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    // A server without a tile file answers the map probe with 404: expected, shows the placeholder.
    if (m.type() === 'error' && !m.location().url.includes('/tiles/')) problems.push(`console: ${m.text()} (${m.location().url})`);
  });
  page.on('request', (r) => {
    const url = r.url();
    if (url.startsWith('data:') || url.startsWith('blob:')) return;
    if (new URL(url).origin !== origin) problems.push(`third-party request: ${url}`);
  });
  return problems;
}

test('gallery renders every component and makes no third-party request', async ({ page, baseURL }) => {
  const problems = watch(page, baseURL as string);
  await page.goto('/__gallery');
  await expect(page.getByRole('heading', { level: 1, name: 'Component gallery' })).toBeVisible();

  // A sample of components in their states.
  await expect(page.getByRole('group', { name: 'Time range' })).toBeVisible();
  await expect(page.getByRole('group', { name: 'Filter by type' })).toBeVisible();
  await expect(page.getByRole('img', { name: 'Energy level 68 out of 100' })).toBeVisible();
  await expect(page.getByRole('img', { name: 'Overnight HRV over 4 weeks' })).toBeVisible();
  await expect(page.getByText('unavailable').first()).toBeVisible();
  await expect(page.locator('.uplot canvas').first()).toBeVisible();
  await expect(page.getByRole('img', { name: 'Route map' }).first()).toBeVisible();

  // Fonts are self-hosted and actually load.
  const fonts = await page.evaluate(async () => {
    await document.fonts.ready;
    return Array.from(document.fonts)
      .filter((f) => f.status === 'loaded')
      .map((f) => f.family.replace(/["']/g, ''));
  });
  expect(fonts).toEqual(expect.arrayContaining(['Inter', 'Space Grotesk']));

  expect(problems).toEqual([]);
});

test('gallery has no serious accessibility violations (incl. 4.5:1 contrast)', async ({ page }) => {
  await page.goto('/__gallery');
  await expect(page.locator('.uplot canvas').first()).toBeVisible();
  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  const summary = results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).slice(0, 3).join(' | ')}`);
  expect(summary).toEqual([]);
});

const ME = { authenticated: true, username: 'demo', role: 'user', timezone: 'Europe/Copenhagen', sample: false };

test('login page logs in with username, password and code via POST /api/login', async ({ page, baseURL }) => {
  const problems = watch(page, baseURL as string);
  const calls: string[] = [];
  let loginHeaders: Record<string, string> = {};
  let loginBody = '';
  await page.route('**/api/login', async (route) => {
    const req = route.request();
    calls.push(`${req.method()} /api/login`);
    loginHeaders = req.headers();
    loginBody = req.postData() ?? '';
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ step: 'done', ...ME }) });
  });
  await page.route('**/api/me', (route) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(ME) }));

  await page.goto('/login');
  await expect(page.getByRole('heading', { name: 'Nimrag Connect' })).toBeVisible();
  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(results.violations.map((v) => v.id)).toEqual([]);

  const submit = page.getByRole('button', { name: 'Log in' });
  await expect(submit).toBeDisabled();
  await page.getByLabel('Username').fill('Demo');
  await page.getByLabel('Password').fill('correct horse');
  await page.getByLabel('6-digit code').fill('123456');
  await submit.click();

  await expect(page).toHaveURL(/\/$/);
  expect(calls).toEqual(['POST /api/login']);
  expect(loginHeaders['x-nimrag']).toBe('1');
  expect(JSON.parse(loginBody)).toEqual({ username: 'Demo', password: 'correct horse', code: '123456' });
  expect(problems.filter((p) => !p.includes('/api/'))).toEqual([]);
});

test('login shows one generic message for a wrong username, password or code and stays on the page', async ({ page }) => {
  await page.route('**/api/login', (route) => route.fulfill({ status: 401, contentType: 'application/json', body: '{"error":"wrong username, password or code"}' }));
  await page.goto('/login');
  await page.getByLabel('Username').fill('demo');
  await page.getByLabel('Password').fill('nope');
  await page.getByRole('button', { name: 'Log in' }).click();
  await expect(page.getByRole('alert')).toContainText('Wrong username, password or code.');
  await expect(page).toHaveURL(/\/login/);
});

for (const vp of [
  { name: 'phone 390', width: 390, height: 844 },
  { name: 'desktop 1360', width: 1360, height: 1000 },
]) {
  test(`login enrol step (QR code, secret, new password) is accessible and draws the QR as inline SVG at ${vp.name}`, async ({ page, baseURL }) => {
    await page.setViewportSize({ width: vp.width, height: vp.height });
    const problems = watch(page, baseURL as string);
    const uri = 'otpauth://totp/Nimrag%20Connect:anna?secret=KRSXG5CTMVRXEZLUKRSXG5CTMVRXEZLU&issuer=Nimrag%20Connect';
    await page.route('**/api/login', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ step: 'enrol2fa', pending_token: 'p', otpauth_uri: uri, secret: 'KRSXG5CTMVRXEZLUKRSXG5CTMVRXEZLU', must_change_password: true }),
      }),
    );
    await page.goto('/login');
    await page.getByLabel('Username').fill('anna');
    await page.getByLabel('Password').fill('a temporary password');
    await page.getByRole('button', { name: 'Log in' }).click();

    const qr = page.getByRole('img', { name: /QR code/ });
    await expect(qr).toBeVisible();
    expect(await qr.evaluate((el) => el.tagName.toLowerCase())).toBe('svg');
    await expect(page.getByLabel('Secret key')).toHaveValue('KRSXG5CTMVRXEZLUKRSXG5CTMVRXEZLU');
    await expect(page.getByLabel('New password', { exact: true })).toBeVisible();
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).slice(0, 3).join(' | ')}`)).toEqual([]);
    // The secret never leaves the page: no request to another origin.
    expect(problems.filter((p) => p.startsWith('third-party'))).toEqual([]);
  });
}
