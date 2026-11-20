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

/** Today's date in an IANA time zone ("2026-11-20"); the browser's own day when the zone is unknown. */
export function todayIn(timeZone: string | null | undefined): string {
  if (!timeZone) return browserToday();
  try {
    return new Intl.DateTimeFormat('en-CA', { timeZone, year: 'numeric', month: '2-digit', day: '2-digit' }).format(new Date());
  } catch {
    return browserToday();
  }
}

/** "Nov 14": month first, as the status pages write their periods. */
export function fmtMonthDay(day: string): string {
  return new Intl.DateTimeFormat('en-US', { timeZone: 'UTC', month: 'short', day: 'numeric' }).format(new Date(`${day}T00:00:00Z`));
}

/** "Sep": the short month name, same spelling as the rest of the status pages. */
export function fmtMonthShort(day: string): string {
  return new Intl.DateTimeFormat('en-US', { timeZone: 'UTC', month: 'short' }).format(new Date(`${day}T00:00:00Z`));
}

/** "S": one letter per month, for a row of twelve. */
export function fmtMonthLetter(day: string): string {
  return new Intl.DateTimeFormat('en-US', { timeZone: 'UTC', month: 'narrow' }).format(new Date(`${day}T00:00:00Z`));
}

/** "Thu, Nov 19" */
export function fmtDayLong(day: string): string {
  return new Intl.DateTimeFormat('en-US', { timeZone: 'UTC', weekday: 'short', month: 'short', day: 'numeric' }).format(new Date(`${day}T00:00:00Z`));
}

/** "Nov 14 – Nov 20"; "Nov 19" for one day; with the years when the period crosses a year or is long. */
export function fmtPeriod(from: string, to: string): string {
  if (from === to) return fmtMonthDay(from);
  const longPeriod = (Date.parse(to) - Date.parse(from)) / 86_400_000 > 300;
  if (from.slice(0, 4) === to.slice(0, 4) && !longPeriod) return `${fmtMonthDay(from)} – ${fmtMonthDay(to)}`;
  const withYear = (day: string) => `${fmtMonthDay(day)}, ${day.slice(0, 4)}`;
  return `${withYear(from)} – ${withYear(to)}`;
}

/** "Nov 15" for one day, "Nov 15 – 16" inside a month, "Oct 30 – Nov 2" across months. */
export function fmtRunDates(from: string, to: string): string {
  if (from === to) return fmtMonthDay(from);
  if (from.slice(0, 7) === to.slice(0, 7)) return `${fmtMonthDay(from)} – ${Number(to.slice(8))}`;
  return `${fmtMonthDay(from)} – ${fmtMonthDay(to)}`;
}
