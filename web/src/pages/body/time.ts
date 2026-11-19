import { fmtShortDay } from '../../lib/dates';

/**
 * The server writes timestamps as RFC 3339 in the owner's timezone ("2026-11-20T09:00:00+01:00").
 * The date and the clock time are read from the text itself, so the browser's own timezone never
 * moves them.
 */
export function localParts(rfc: string | null | undefined): { day: string; time: string } | null {
  if (!rfc || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(rfc)) return null;
  return { day: rfc.slice(0, 10), time: rfc.slice(11, 16) };
}

/** "Fri 20 Nov 09:00" */
export function fmtLocalStamp(rfc: string | null | undefined): string | null {
  const p = localParts(rfc);
  return p ? `${fmtShortDay(p.day)} ${p.time}` : null;
}
