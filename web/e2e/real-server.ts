// Shared by the real-server Playwright project (real.setup.ts, real.teardown.ts, real.spec.ts).
// The project runs the app against the actual `nimrag-server` binary with dev-seed data, so the
// pages are exercised through the real API instead of mocks.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));

export const webDir = path.resolve(here, '..');
export const repoRoot = path.resolve(webDir, '..');

/** Fixed, so nobody else's dev server is touched: 18121 is this stage's port. */
export const PORT = Number(process.env['NIMRAG_E2E_PORT'] ?? 18121);
export const ORIGIN = `http://127.0.0.1:${PORT}`;

/** Where the second, empty server answers: the first-run setup page is tested against it. */
export const SETUP_PORT = PORT + 1;
export const SETUP_ORIGIN = `http://127.0.0.1:${SETUP_PORT}`;

/**
 * The seeded accounts. `dev-seed` takes their passwords and 2FA secrets from the environment
 * (NIMRAG_SEED_*), so the tests know them up front. Passwords need 10 characters or more; the
 * secrets are base32.
 */
export interface Persona {
  username: string;
  password: string;
  /** Base32 TOTP secret. */
  totp: string;
}
/** Role admin, no data: sets the server up and manages users. */
export const ADMIN: Persona = { username: 'admin', password: 'nimrag-e2e-admin-password', totp: 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ' };
/** Role user: owns all the sample data. */
export const DEMO: Persona = { username: 'demo', password: 'nimrag-e2e-demo-password', totp: 'KRSXG5CTMVRXEZLUKRSXG5CTMVRXEZLU' };

// Outside Playwright's output folder, which is emptied at the start of every run.
export const stateDir = path.join(webDir, '.e2e');
/** What the teardown needs to clean up: server pid and temp data dir. */
export const serverFile = path.join(stateDir, 'server.json');
/** Browser storage state with the demo user's session cookie (the default of the real project). */
export const sessionFile = path.join(stateDir, 'session-demo.json');
/** The same for the admin. */
export const adminSessionFile = path.join(stateDir, 'session-admin.json');
/** Ids and dates of the seeded data, for the routes that need one. */
export const fixtureFile = path.join(stateDir, 'fixture.json');

export interface ServerProcess {
  pid: number;
  dataDir: string;
}

export interface ServerState extends ServerProcess {
  /** The binary, for CLI helpers (`token create`). */
  bin: string;
  /** The empty server behind SETUP_ORIGIN. */
  setup?: ServerProcess;
}

export interface Fixture {
  /** The owner's today (YYYY-MM-DD). */
  today: string;
  month: string;
  /** Yesterday, a night that has sleep. */
  night: string;
  runId: string;
  strengthId: string;
}

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/** Only ever stop a process that really is our server (a pid can be reused after a crash). */
function isOurServer(pid: number): boolean {
  try {
    return fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8').includes('nimrag-server');
  } catch {
    return false;
  }
}

async function stop(proc: ServerProcess): Promise<void> {
  if (alive(proc.pid) && isOurServer(proc.pid)) {
    process.kill(proc.pid, 'SIGTERM');
    const deadline = Date.now() + 10_000;
    while (alive(proc.pid) && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 100));
    if (alive(proc.pid)) process.kill(proc.pid, 'SIGKILL');
  }
  fs.rmSync(proc.dataDir, { recursive: true, force: true });
}

/** Stops the servers started by the setup and deletes their data dirs. Safe to call when nothing runs. */
export async function stopServer(): Promise<void> {
  if (!fs.existsSync(serverFile)) return;
  const state = JSON.parse(fs.readFileSync(serverFile, 'utf8')) as ServerState;
  await stop(state);
  if (state.setup) await stop(state.setup);
  for (const f of [serverFile, sessionFile, adminSessionFile, fixtureFile, path.join(stateDir, 'totp-steps.json'), path.join(stateDir, 'layout-tokens.done')]) fs.rmSync(f, { force: true });
  fs.rmSync(path.join(stateDir, 'layout-tokens.lock'), { recursive: true, force: true });
}

export function readServerState(): ServerState {
  return JSON.parse(fs.readFileSync(serverFile, 'utf8')) as ServerState;
}
