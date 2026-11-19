import { execFileSync, spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { expect, test as setup } from '@playwright/test';
import { fixtureFile, ORIGIN, PASSWORD, PORT, repoRoot, serverFile, sessionFile, stateDir, stopServer, webDir, type Fixture, type ServerState } from './real-server';

// Setup of the real-server project:
//   1. build the web app (skipped when web/dist is newer than every source file)
//   2. build nimrag-server (skipped with NIMRAG_SERVER_BIN=/path/to/binary)
//   3. seed a temp data dir (dev-seed), set the owner password
//   4. start `nimrag-server serve --insecure-cookies` on 127.0.0.1:18121
//   5. log in and keep the session for the tests
// real.teardown.ts stops the server and deletes the data dir.

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

async function waitForHealth(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      const res = await fetch(`${ORIGIN}/api/health`);
      if (res.ok) return;
    } catch {
      // not listening yet
    }
    if (Date.now() > deadline) throw new Error(`nimrag-server did not answer on ${ORIGIN} within ${timeoutMs / 1000} s`);
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
}

setup('start the real server with seeded data', async ({ request }) => {
  setup.setTimeout(15 * 60_000);
  fs.mkdirSync(stateDir, { recursive: true });

  // A run that was killed leaves its server behind: stop it before starting another.
  await stopServer();

  buildWeb();
  const bin = serverBinary();

  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nimrag-e2e-'));
  const env = { ...process.env, SQLX_OFFLINE: 'true' };
  execFileSync(bin, ['dev-seed', '--data-dir', dataDir], { stdio: 'inherit', env });
  const pw = spawnSync(bin, ['user', 'set-password', '--data-dir', dataDir], { input: PASSWORD, encoding: 'utf8', env });
  if (pw.status !== 0) throw new Error(`user set-password failed: ${pw.stderr}`);

  const log = fs.openSync(path.join(stateDir, 'server.log'), 'w');
  const child = spawn(bin, ['serve', '--data-dir', dataDir, '--listen', `127.0.0.1:${PORT}`, '--insecure-cookies'], {
    cwd: repoRoot,
    env,
    detached: true,
    stdio: ['ignore', log, log],
  });
  child.unref();
  if (!child.pid) throw new Error('nimrag-server did not start');
  const state: ServerState = { pid: child.pid, dataDir };
  fs.writeFileSync(serverFile, JSON.stringify(state));
  await waitForHealth(30_000);

  const login = await request.post(`${ORIGIN}/api/login`, { data: { password: PASSWORD }, headers: { 'X-Nimrag': '1' } });
  expect(login.ok(), 'login with the seeded owner password').toBe(true);
  await request.storageState({ path: sessionFile });

  const today = (await (await request.get(`${ORIGIN}/api/today`)).json()) as { date: string };
  const list = (await (await request.get(`${ORIGIN}/api/activities?limit=100`)).json()) as { items: { id: string | number; kind: string }[] };
  const run = list.items.find((a) => a.kind === 'run');
  const strength = list.items.find((a) => a.kind === 'strength');
  expect(run, 'the seed has a run').toBeDefined();
  expect(strength, 'the seed has a strength session').toBeDefined();
  const [y, m, d] = today.date.split('-').map(Number) as [number, number, number];
  const night = new Date(Date.UTC(y, m - 1, d - 1)).toISOString().slice(0, 10);
  const fixture: Fixture = { today: today.date, month: today.date.slice(0, 7), night, runId: String(run?.id), strengthId: String(strength?.id) };
  fs.writeFileSync(fixtureFile, JSON.stringify(fixture));
});
