import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { expect, test as setup } from '@playwright/test';
import {
  ADMIN,
  adminSessionFile,
  DEMO,
  fixtureFile,
  ORIGIN,
  PORT,
  repoRoot,
  serverFile,
  SETUP_ORIGIN,
  SETUP_PORT,
  sessionFile,
  stateDir,
  stopServer,
  webDir,
  type Fixture,
  type Persona,
  type ServerProcess,
  type ServerState,
} from './real-server';
import { nextTotp, resetTotpSteps } from './totp';

// Setup of the real-server project:
//   1. build the web app (skipped when web/dist is newer than every source file)
//   2. build nimrag-server (skipped with NIMRAG_SERVER_BIN=/path/to/binary)
//   3. seed a temp data dir (dev-seed): the admin `admin` (no data) and the user `demo` (all the
//      sample data), both with 2FA, with the passwords and secrets of real-server.ts
//   4. start `nimrag-server serve --insecure-cookies` on 127.0.0.1:18121
//   5. start a second server on an EMPTY data dir (127.0.0.1:18122) for the first-run setup page
//   6. log each of the two accounts in once, through the real API, and keep the two sessions
// real.teardown.ts stops both servers and deletes their data dirs.
//
// Two-factor: the server refuses a code whose 30 s step is not newer than the last accepted one
// for that user, so a persona logs in ONCE here and the specs share the saved session. A spec that
// logs in again takes its code from `nextTotp` (totp.ts), which hands out strictly newer steps.

function newestMtime(target: string): number {
  const stat = fs.statSync(target, { throwIfNoEntry: false });
  if (!stat) return 0;
  if (!stat.isDirectory()) return stat.mtimeMs;
  return fs
    .readdirSync(target)
    .map((name) => newestMtime(path.join(target, name)))
    .reduce((a, b) => Math.max(a, b), stat.mtimeMs);
}

function buildWeb() {
  const dist = fs.statSync(path.join(webDir, 'dist', 'index.html'), { throwIfNoEntry: false });
  const sources = ['src', 'public', 'index.html', 'vite.config.ts', 'package.json', 'package-lock.json', 'tsconfig.app.json'].map((p) => newestMtime(path.join(webDir, p)));
  if (dist && dist.mtimeMs > Math.max(...sources)) return;
  execFileSync('npm', ['run', 'build'], { cwd: webDir, stdio: 'inherit' });
}

function serverBinary(): string {
  const given = process.env['NIMRAG_SERVER_BIN'];
  if (given) return given;
  const env = { ...process.env, SQLX_OFFLINE: 'true' };
  execFileSync('cargo', ['build', '-p', 'nimrag-server'], { cwd: repoRoot, stdio: 'inherit', env });
  const meta = execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], { cwd: repoRoot, env, maxBuffer: 64 * 1024 * 1024 });
  const target = (JSON.parse(meta.toString()) as { target_directory: string }).target_directory;
  return path.join(target, 'debug', 'nimrag-server');
}

function start(bin: string, dataDir: string, port: number, logName: string): ServerProcess {
  const log = fs.openSync(path.join(stateDir, logName), 'w');
  const child = spawn(bin, ['serve', '--data-dir', dataDir, '--listen', `127.0.0.1:${port}`, '--insecure-cookies'], {
    cwd: repoRoot,
    env: { ...process.env, SQLX_OFFLINE: 'true' },
    detached: true,
    stdio: ['ignore', log, log],
  });
  child.unref();
  if (!child.pid) throw new Error('nimrag-server did not start');
  return { pid: child.pid, dataDir };
}

async function waitForHealth(origin: string, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      const res = await fetch(`${origin}/api/health`);
      if (res.ok) return;
    } catch {
      // not listening yet
    }
    if (Date.now() > deadline) throw new Error(`nimrag-server did not answer on ${origin} within ${timeoutMs / 1000} s`);
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
}

setup('start the real servers with seeded data', async ({ playwright }) => {
  setup.setTimeout(15 * 60_000);
  fs.mkdirSync(stateDir, { recursive: true });

  // A run that was killed leaves its servers behind: stop them before starting others.
  await stopServer();
  resetTotpSteps();

  buildWeb();
  const bin = serverBinary();

  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nimrag-e2e-'));
  const env = {
    ...process.env,
    SQLX_OFFLINE: 'true',
    NIMRAG_SEED_ADMIN_PASSWORD: ADMIN.password,
    NIMRAG_SEED_ADMIN_TOTP: ADMIN.totp,
    NIMRAG_SEED_USER_PASSWORD: DEMO.password,
    NIMRAG_SEED_USER_TOTP: DEMO.totp,
  };
  execFileSync(bin, ['dev-seed', '--data-dir', dataDir], { stdio: 'inherit', env });

  const main = start(bin, dataDir, PORT, 'server.log');
  const state: ServerState = { ...main, bin };
  fs.writeFileSync(serverFile, JSON.stringify(state));
  const setupDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nimrag-e2e-setup-'));
  state.setup = start(bin, setupDir, SETUP_PORT, 'server-setup.log');
  fs.writeFileSync(serverFile, JSON.stringify(state));
  await Promise.all([waitForHealth(ORIGIN, 30_000), waitForHealth(SETUP_ORIGIN, 30_000)]);

  // The empty server really is waiting for its first admin, the seeded one is not.
  expect(await (await fetch(`${SETUP_ORIGIN}/api/setup`)).json()).toEqual({ needed: true });
  expect(await (await fetch(`${ORIGIN}/api/setup`)).json()).toEqual({ needed: false });

  async function signIn(who: Persona, file: string) {
    const context = await playwright.request.newContext({ baseURL: ORIGIN });
    const login = await context.post('/api/login', { data: { username: who.username, password: who.password, code: await nextTotp(who.totp) }, headers: { 'X-Nimrag': '1' } });
    expect(login.ok(), `login of ${who.username}: ${login.status()}`).toBe(true);
    expect(((await login.json()) as { step: string }).step).toBe('done');
    await context.storageState({ path: file });
    return context;
  }
  const admin = await signIn(ADMIN, adminSessionFile);
  expect(await (await admin.get('/api/me')).json()).toMatchObject({ username: 'admin', role: 'admin' });
  // An admin has no data: every data endpoint says 403.
  expect((await admin.get('/api/today')).status()).toBe(403);
  await admin.dispose();

  const demo = await signIn(DEMO, sessionFile);
  expect(await (await demo.get('/api/me')).json()).toMatchObject({ username: 'demo', role: 'user' });
  const today = (await (await demo.get('/api/today')).json()) as { date: string };
  const list = (await (await demo.get('/api/activities?limit=100')).json()) as { items: { id: string | number; kind: string }[] };
  const run = list.items.find((a) => a.kind === 'run');
  const strength = list.items.find((a) => a.kind === 'strength');
  const walk = list.items.find((a) => a.kind === 'walk');
  expect(run, 'the seed has a run').toBeDefined();
  expect(strength, 'the seed has a strength session').toBeDefined();
  expect(walk, 'the seed has a walk').toBeDefined();
  const [y, m, d] = today.date.split('-').map(Number) as [number, number, number];
  const night = new Date(Date.UTC(y, m - 1, d - 1)).toISOString().slice(0, 10);
  // One saved workout for the workout pages: a warm-up, a repeat block, a cool-down. Saving works with
  // and without the FIT profile (only "send to watch" needs it), so the seed does not depend on the build.
  const created = await demo.post('/api/workouts', {
    headers: { 'X-Nimrag': '1' },
    data: {
      name: 'Intervals 4 x 400 m',
      sport: 'running',
      note: 'Seeded for the page checks',
      nodes: [
        { type: 'step', intensity: 'warmup', duration: { kind: 'time', seconds: 600 }, target: { kind: 'heart_rate_zone', zone: 2 } },
        {
          type: 'repeat',
          count: 4,
          steps: [
            { intensity: 'active', duration: { kind: 'distance', meters: 400 }, target: { kind: 'pace', fast_s_per_km: 270, slow_s_per_km: 300 } },
            { intensity: 'recovery', duration: { kind: 'time', seconds: 90 }, target: { kind: 'none' } },
          ],
        },
        { type: 'step', intensity: 'cooldown', duration: { kind: 'open' }, target: { kind: 'none' } },
      ],
    },
  });
  expect(created.status(), 'saving the seeded workout').toBe(201);
  const workout = (await created.json()) as { id: number };
  const fixture: Fixture = {
    today: today.date,
    month: today.date.slice(0, 7),
    night,
    runId: String(run?.id),
    strengthId: String(strength?.id),
    walkId: String(walk?.id),
    workoutId: String(workout.id),
  };
  fs.writeFileSync(fixtureFile, JSON.stringify(fixture));
  await demo.dispose();
});
