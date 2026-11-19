// Renders public/icon.svg to the PNG icons the PWA manifest and iOS need.
// Run when icon.svg changes: node scripts/make-icons.mjs (needs `npx playwright install chromium`).
import { chromium } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const svg = readFileSync(fileURLToPath(new URL('../public/icon.svg', import.meta.url)), 'utf8');
const targets = [
  ['icon-192.png', 192],
  ['icon-512.png', 512],
  ['apple-touch-icon.png', 180],
];

const browser = await chromium.launch();
try {
  for (const [name, size] of targets) {
    const page = await browser.newPage({ viewport: { width: size, height: size } });
    await page.setContent(`<body style="margin:0;background:transparent">${svg.replace('<svg ', `<svg width="${size}" height="${size}" `)}</body>`);
    await page.screenshot({ path: fileURLToPath(new URL(`../public/${name}`, import.meta.url)), omitBackground: true });
    await page.close();
  }
} finally {
  await browser.close();
}
