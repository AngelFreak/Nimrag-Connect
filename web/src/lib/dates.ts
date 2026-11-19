// Calendar-date helpers on "YYYY-MM-DD" and "YYYY-MM" strings. Days are local days of the
// owner's timezone, decided by the server; the browser never reinterprets them, so all
// arithmetic here is done in UTC to stay independent of the browser's own timezone.

const ISO_DAY = /^\d{4}-\d{2}-\d{2}$/;
const ISO_MONTH = /^\d{4}-\d{2}$/;

export function isIsoDay(s: string | undefined): s is string {
  if (!s || !ISO_DAY.test(s)) return false;
  const d = new Date(`${s}T00:00:00Z`);
  return !Number.isNaN(d.getTime()) && d.toISOString().startsWith(s);
}

export function isIsoMonth(s: string | undefined): s is string {
  return !!s && ISO_MONTH.test(s) && Number(s.slice(5)) >= 1 && Number(s.slice(5)) <= 12;
}

export function addDays(day: string, n: number): string {
  const d = new Date(`${day}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + n);
  return d.toISOString().slice(0, 10);
}

export function addMonths(month: string, n: number): string {
  const y = Number(month.slice(0, 4));
  const m = Number(month.slice(5)) - 1 + n;
  const d = new Date(Date.UTC(y, m, 1));
  return d.toISOString().slice(0, 7);
}

/** Today in the browser's zone, only as a fallback until the API says which day "today" is. */
export function browserToday(): string {
  const d = new Date();
  const mm = String(d.getMonth() + 1).padStart(2, '0');
  const dd = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${mm}-${dd}`;
}

export function fmtDay(day: string, opts: Intl.DateTimeFormatOptions): string {
  return new Intl.DateTimeFormat('en-GB', { timeZone: 'UTC', ...opts }).format(new Date(`${day}T00:00:00Z`));
}

/** "Wed 18 Nov" */
export function fmtShortDay(day: string): string {
  return fmtDay(day, { weekday: 'short', day: 'numeric', month: 'short' }).replace(',', '');
}

/** "November 2026" */
export function fmtMonth(month: string): string {
  return fmtDay(`${month}-01`, { month: 'long', year: 'numeric' });
}
