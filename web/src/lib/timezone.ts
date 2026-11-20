// Time zone helpers for the Settings picker and the "your days follow UTC" notice.

/** Used when the browser cannot list its zones (`Intl.supportedValuesOf` is missing). */
export const FALLBACK_ZONES: readonly string[] = [
  'UTC',
  'Europe/London',
  'Europe/Dublin',
  'Europe/Lisbon',
  'Europe/Madrid',
  'Europe/Paris',
  'Europe/Brussels',
  'Europe/Amsterdam',
  'Europe/Berlin',
  'Europe/Copenhagen',
  'Europe/Stockholm',
  'Europe/Oslo',
  'Europe/Zurich',
  'Europe/Rome',
  'Europe/Vienna',
  'Europe/Prague',
  'Europe/Warsaw',
  'Europe/Athens',
  'Europe/Helsinki',
  'Europe/Istanbul',
  'Europe/Moscow',
  'Africa/Cairo',
  'Africa/Johannesburg',
  'Asia/Dubai',
  'Asia/Kolkata',
  'Asia/Bangkok',
  'Asia/Singapore',
  'Asia/Shanghai',
  'Asia/Tokyo',
  'Asia/Seoul',
  'Australia/Perth',
  'Australia/Sydney',
  'Pacific/Auckland',
  'America/Sao_Paulo',
  'America/Argentina/Buenos_Aires',
  'America/New_York',
  'America/Chicago',
  'America/Denver',
  'America/Los_Angeles',
  'America/Anchorage',
  'Pacific/Honolulu',
];

/** The browser's own IANA zone ("Europe/Copenhagen"); `null` when it cannot say. */
export function browserZone(): string | null {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || null;
  } catch {
    return null;
  }
}

/** True for the UTC spellings a server or browser may report. */
export function isUtcZone(zone: string | null | undefined): boolean {
  return zone === 'UTC' || zone === 'Etc/UTC' || zone === 'Etc/UCT' || zone === 'Etc/Zulu' || zone === 'Z';
}

/** Two zone names that mean the same thing for this purpose (all UTC spellings are one). */
export function sameZone(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  return a === b || (isUtcZone(a) && isUtcZone(b));
}

/**
 * The zone to offer as "Use <zone>": the browser's zone when it differs from `saved`; `null` when
 * they are the same, or the browser's zone is unknown or not one the server can save.
 */
export function suggestedZone(saved: string | null | undefined): string | null {
  const here = browserZone();
  if (!here || sameZone(saved, here)) return null;
  return here;
}

/** All zone names the browser knows, or a short list; always sorted and without duplicates. */
export function zoneNames(): string[] {
  let names: string[] = [];
  try {
    names = (Intl as unknown as { supportedValuesOf?: (key: string) => string[] }).supportedValuesOf?.('timeZone') ?? [];
  } catch {
    names = [];
  }
  return [...new Set(names.length > 0 ? names : FALLBACK_ZONES)].sort();
}

/** The picker's options: every zone, plus the saved zone and the browser's zone even when unlisted ("UTC"). */
export function zoneOptions(...extra: (string | null | undefined)[]): string[] {
  const set = new Set(zoneNames());
  set.add('UTC');
  for (const z of extra) if (z) set.add(z);
  return [...set].sort();
}
