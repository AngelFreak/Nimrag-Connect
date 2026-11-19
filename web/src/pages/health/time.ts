// Timestamps from the API are RFC 3339 strings in the owner's timezone ("2026-11-18T23:32:41+01:00").
// The wall-clock part is already the owner's local time, so clock labels are read from the text
// and never reinterpreted in the browser's zone; the instant (epoch) is used only for placement.

export function epochMs(ts: string): number {
  return Date.parse(ts);
}

/** "23:32", the local wall clock of the timestamp; null when the text is not a timestamp. */
export function clock(ts: string | null | undefined): string | null {
  if (!ts || ts.length < 16 || ts[10] !== 'T') return null;
  return ts.slice(11, 16);
}

/** Offset of the timestamp in minutes east of UTC (+01:00 is 60). */
export function offsetMin(ts: string): number {
  if (ts.endsWith('Z')) return 0;
  const m = /([+-])(\d{2}):?(\d{2})$/.exec(ts);
  if (!m) return 0;
  const n = Number(m[2]) * 60 + Number(m[3]);
  return m[1] === '-' ? -n : n;
}

/** Wall-clock "HH:MM" of an instant in a zone given by its offset. */
export function clockAt(ms: number, offset: number): string {
  const d = new Date(ms + offset * 60_000);
  return `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}`;
}

/** Epoch of local midnight at the start of `date`, in the zone that `sampleTs` is written in. */
export function dayStartMs(date: string, sampleTs: string): number {
  return Date.parse(`${date}T00:00:00Z`) - offsetMin(sampleTs) * 60_000;
}

/** Whole days from `from` to `to` (both "YYYY-MM-DD"). */
export function daysBetween(from: string, to: string): number {
  return Math.round((Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / 86_400_000);
}
