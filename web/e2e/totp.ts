// A tiny RFC 6238 TOTP (HMAC-SHA1, 6 digits, 30 s steps) for the real-server tests, and the
// bookkeeping that keeps them out of the server's replay protection.
//
// The server accepts a code for a step within +/-1 of its clock, and only when that step is
// newer than the last one it accepted for the user. So two logins of the same user inside one
// 30 s step fail the second time. `nextTotp` hands out, per secret, strictly increasing steps:
// the first login of a run uses the current step, the next one the following step (still inside
// the +/-1 skew), and when a test would need a step more than one ahead of the clock it waits
// until the clock has caught up. The last step per secret lives in a file under a directory
// lock, so tests in different workers share it. Every login of a seeded persona in the e2e
// suite must get its code from here.

import { createHmac } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { stateDir } from './real-server';

const STEP_MS = 30_000;
const stepsFile = path.join(stateDir, 'totp-steps.json');
const lockDir = path.join(stateDir, 'totp-steps.lock');

const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';

function base32Decode(text: string): Buffer {
  const clean = text.replace(/[\s=-]/g, '').toUpperCase();
  let bits = 0;
  let value = 0;
  const out: number[] = [];
  for (const ch of clean) {
    const index = ALPHABET.indexOf(ch);
    if (index < 0) throw new Error(`not base32: ${ch}`);
    value = (value << 5) | index;
    bits += 5;
    if (bits >= 8) {
      out.push((value >>> (bits - 8)) & 0xff);
      bits -= 8;
    }
  }
  return Buffer.from(out);
}

/** The code for one 30 s step (`step` = unix time / 30). */
export function totpForStep(secret: string, step: number): string {
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(step));
  const mac = createHmac('sha1', base32Decode(secret)).update(counter).digest();
  const offset = mac[mac.length - 1]! & 0x0f;
  const binary = ((mac[offset]! & 0x7f) << 24) | (mac[offset + 1]! << 16) | (mac[offset + 2]! << 8) | mac[offset + 3]!;
  return String(binary % 1_000_000).padStart(6, '0');
}

export function currentStep(now = Date.now()): number {
  return Math.floor(now / STEP_MS);
}

/** The code for the current step: for a secret nobody has used yet (a fresh enrolment). */
export function totpNow(secret: string): string {
  return totpForStep(secret, currentStep());
}

async function withLock<T>(work: () => T): Promise<T> {
  fs.mkdirSync(stateDir, { recursive: true });
  const deadline = Date.now() + 30_000;
  for (;;) {
    try {
      fs.mkdirSync(lockDir);
      break;
    } catch {
      if (Date.now() > deadline) fs.rmSync(lockDir, { recursive: true, force: true });
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  }
  try {
    return work();
  } finally {
    fs.rmSync(lockDir, { recursive: true, force: true });
  }
}

/** Forgets every recorded step (a new server has new accounts). */
export function resetTotpSteps(): void {
  fs.rmSync(stepsFile, { force: true });
  fs.rmSync(lockDir, { recursive: true, force: true });
}

/**
 * A code the server will accept for this secret, newer than every code handed out before for it.
 * Waits when the next free step is more than one step ahead of the clock.
 */
export async function nextTotp(secret: string): Promise<string> {
  const step = await withLock(() => {
    const steps = fs.existsSync(stepsFile) ? (JSON.parse(fs.readFileSync(stepsFile, 'utf8')) as Record<string, number>) : {};
    const next = Math.max(currentStep(), (steps[secret] ?? 0) + 1);
    steps[secret] = next;
    fs.writeFileSync(stepsFile, JSON.stringify(steps));
    return next;
  });
  // Valid from the start of step - 1; the server's clock is this machine's, so no extra margin.
  const validFrom = (step - 1) * STEP_MS + 250;
  const wait = validFrom - Date.now();
  if (wait > 0) await new Promise((resolve) => setTimeout(resolve, wait));
  return totpForStep(secret, step);
}
