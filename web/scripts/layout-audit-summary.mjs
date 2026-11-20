// Summarises web/.e2e/layout-audit.jsonl (written by e2e/real.layout.spec.ts): findings per page,
// by kind, and the distinct offending elements. Usage:
//   node scripts/layout-audit-summary.mjs [--elements] [--kind=past-edge] [file]
// Delete the file before a run to start a fresh record.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const showElements = args.includes('--elements');
const kindFilter = args.find((a) => a.startsWith('--kind='))?.slice(7);
const file = args.find((a) => !a.startsWith('--')) ?? path.join(here, '..', '.e2e', 'layout-audit.jsonl');
if (!fs.existsSync(file)) {
  console.error(`no audit record at ${file}`);
  process.exit(1);
}

// Later lines replace earlier ones for the same page and size (retries, reruns).
const latest = new Map();
for (const line of fs.readFileSync(file, 'utf8').split('\n').filter(Boolean)) {
  const row = JSON.parse(line);
  latest.set(`${row.project}|${row.page}|${row.size}`, row);
}

const byPage = new Map();
let total = 0;
let measured = 0;
for (const row of latest.values()) {
  measured++;
  const findings = kindFilter ? row.findings.filter((f) => f.kind === kindFilter) : row.findings;
  const entry = byPage.get(row.page) ?? { count: 0, sizes: new Set(), kinds: {}, elements: new Map() };
  for (const f of findings) {
    entry.count++;
    total++;
    entry.kinds[f.kind] = (entry.kinds[f.kind] ?? 0) + 1;
    entry.elements.set(`${f.kind}: ${f.element}`, `${f.detail} @ ${row.size}`);
  }
  if (findings.length > 0) entry.sizes.add(row.size);
  byPage.set(row.page, entry);
}

console.log(`${measured} page and size combinations measured, ${total} findings\n`);
for (const [page, e] of byPage) {
  const kinds = Object.entries(e.kinds)
    .map(([k, n]) => `${k} ${n}`)
    .join(', ');
  console.log(`${e.count === 0 ? 'ok  ' : 'FAIL'} ${page.padEnd(40)} ${String(e.count).padStart(4)}  ${kinds}`);
  if (showElements) for (const [el, detail] of e.elements) console.log(`       ${el}  [${detail}]`);
}
