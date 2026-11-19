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

/** The owner password the setup gives the seeded database (the server wants 10 characters or more). */
export const PASSWORD = 'nimrag-e2e-password';

// Outside Playwright's output folder, which is emptied at the start of every run.
export const stateDir = path.join(webDir, '.e2e');
/** What the teardown needs to clean up: server pid and temp data dir. */
export const serverFile = path.join(stateDir, 'server.json');
/** Browser storage state with the logged-in session cookie. */
export const sessionFile = path.join(stateDir, 'session.json');
/** Ids and dates of the seeded data, for the routes that need one. */
export const fixtureFile = path.join(stateDir, 'fixture.json');

export interface ServerState {
  pid: number;
  dataDir: string;
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

/** Stops the server started by the setup and deletes its data dir. Safe to call when nothing runs. */
export async function stopServer(): Promise<void> {
  if (!fs.existsSync(serverFile)) return;
  const state = JSON.parse(fs.readFileSync(serverFile, 'utf8')) as ServerState;
  if (alive(state.pid) && isOurServer(state.pid)) {
    process.kill(state.pid, 'SIGTERM');
    const deadline = Date.now() + 10_000;
    while (alive(state.pid) && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 100));
    if (alive(state.pid)) process.kill(state.pid, 'SIGKILL');
  }
  fs.rmSync(state.dataDir, { recursive: true, force: true });
  for (const f of [serverFile, sessionFile, fixtureFile]) fs.rmSync(f, { force: true });
}
