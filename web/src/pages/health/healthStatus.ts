import type { HealthMetricHistory } from '../../api/generated/HealthMetricHistory';
import type { HealthMetricKey } from '../../api/generated/HealthMetricKey';
import type { HealthMetricStatus } from '../../api/generated/HealthMetricStatus';
import type { HealthState } from '../../api/generated/HealthState';
import type { HealthStatusResponse } from '../../api/generated/HealthStatusResponse';
import type { BandSeries, ChartMarker, XLabel } from '../../components';
import { addDays, fmtMonthDay } from '../../lib/dates';
import { fmtFixed } from '../../lib/format';

// Health status: last night's overnight values against the person's own usual range (the server
// does the comparison, see server/src/web_api/health_status.rs). These are the words and figures
// the page, the Today card and the tests share. Pure functions.

/** Nights a usual range is built from, and the fewest of them it needs (the same as the server's). */
export const WINDOW_NIGHTS = 28;
export const MIN_NIGHTS = 14;

export const METRIC_ORDER: readonly HealthMetricKey[] = ['hrv', 'resting_hr', 'breathing', 'spo2'];

export const METRIC_NAME: Record<HealthMetricKey, string> = {
  hrv: 'Overnight HRV',
  resting_hr: 'Lowest heart rate asleep',
  breathing: 'Breathing rate asleep',
  spo2: 'Blood oxygen asleep',
};

/** A shorter name for the Today card. */
export const METRIC_SHORT: Record<HealthMetricKey, string> = {
  hrv: 'HRV',
  resting_hr: 'Heart rate',
  breathing: 'Breathing',
  spo2: 'Blood oxygen',
};

export const STATE_WORD: Record<HealthState, string> = {
  in_range: 'In range',
  above: 'Above',
  below: 'Below',
  calibrating: 'Calibrating',
  no_reading: 'No reading',
};

/** The unit as the other pages write it: breaths a minute are "/min", as on the Sleep page. */
export function unitText(unit: string): string {
  return unit === 'brpm' ? '/min' : unit;
}

/** A figure with its unit, as the server gives it: whole when whole ("66 ms"), else one decimal ("63.4 ms", "95.2%"). */
export function fmtMetric(value: number | null | undefined, unit: string): string | null {
  if (value === null || value === undefined || !Number.isFinite(value)) return null;
  const n = Number.isInteger(value) ? String(value) : (fmtFixed(value, 1) as string);
  return unit === '%' ? `${n}%` : `${n} ${unitText(unit)}`;
}

/** "41.0 to 63.4 ms": the usual range, or `null` while calibrating. */
export function rangeText(m: Pick<HealthMetricStatus, 'low' | 'high' | 'unit'>): string | null {
  const lo = fmtFixed(m.low, 1);
  const hi = fmtFixed(m.high, 1);
  if (lo === null || hi === null) return null;
  return m.unit === '%' ? `${lo}% to ${hi}%` : `${lo} to ${hi} ${unitText(m.unit)}`;
}

/** The one line over the page: how many values are outside, in plain words. */
export function summaryLine(r: Pick<HealthStatusResponse, 'has_night' | 'outside' | 'compared' | 'metrics'>): string {
  if (!r.has_night) return 'No night recorded';
  if (r.compared === 0) {
    const calibrating = r.metrics.some((m) => m.state === 'calibrating');
    return calibrating ? 'Still learning your usual range' : 'Nothing measured last night';
  }
  if (r.outside === 0) return r.compared === r.metrics.length ? 'All in your usual range' : `All ${r.compared} compared values are in your usual range`;
  return r.outside === 1 ? '1 value outside your usual range' : `${r.outside} values outside your usual range`;
}

/** The line under the summary: what was compared, or what is missing. */
export function summarySub(r: Pick<HealthStatusResponse, 'has_night' | 'compared' | 'metrics'>): string {
  if (!r.has_night) return 'The watch recorded no sleep for this night, so there is nothing to compare.';
  const calibrating = r.metrics.filter((m) => m.state === 'calibrating');
  if (calibrating.length > 0) {
    const most = Math.max(...calibrating.map((m) => m.nights_needed));
    return `${calibrating.length === 1 ? 'One value needs' : `${calibrating.length} values need`} up to ${most} more ${most === 1 ? 'night' : 'nights'} before a usual range can be shown.`;
  }
  const missing = r.metrics.filter((m) => m.state === 'no_reading');
  if (missing.length > 0) return `${missing.map((m) => METRIC_SHORT[m.key]).join(' and ')} not measured last night.`;
  return `${r.compared} of ${r.metrics.length} values compared with your last 28 nights.`;
}

/** One sentence for a screen reader: the value, the range and where it sits. */
export function metricSentence(m: HealthMetricStatus): string {
  const name = METRIC_NAME[m.key];
  const value = fmtMetric(m.value, m.unit);
  if (m.state === 'no_reading' || value === null) return `${name}: not measured last night.`;
  const range = rangeText(m);
  if (m.state === 'calibrating' || range === null) {
    return `${name}: ${value}. Calibrating, ${m.nights_needed} more ${m.nights_needed === 1 ? 'night' : 'nights'} needed for a usual range.`;
  }
  const where = m.state === 'in_range' ? 'inside' : m.state === 'above' ? 'above' : 'below';
  // Read aloud, "/min" says nothing: breaths a minute in words.
  const spoken = (t: string) => t.replace(/ \/min$/, ' breaths a minute');
  return `${name}: ${spoken(value)}, ${where} your usual range of ${spoken(range)}.`;
}

/**
 * Where the value sits on a bar: the axis runs a little beyond the range and the value, so both
 * stay inside it. Shares are 0 to 1 from the left.
 */
export function stripLayout(m: Pick<HealthMetricStatus, 'value' | 'low' | 'high'>): { lo: number; hi: number; at: number } | null {
  if (m.value === null || m.low === null || m.high === null) return null;
  const min = Math.min(m.low, m.value);
  const max = Math.max(m.high, m.value);
  const pad = Math.max((max - min) * 0.25, 0.5);
  const a = min - pad;
  const b = max + pad;
  const at = (v: number) => (v - a) / (b - a);
  return { lo: at(m.low), hi: at(m.high), at: at(m.value) };
}

export interface HistoryChart {
  x: number[];
  values: (number | null)[];
  band: BandSeries;
  markers: (ChartMarker | null)[];
  xLabels: XLabel[];
  summary: string;
  /** Nights with a value. */
  measured: number;
  /** Whether any night has a usual range to draw. */
  hasBand: boolean;
}

/** The chart of one metric over the nights of `from` to `to`: the values, the changing band and a marker on each night outside it. */
export function historyChart(h: HealthMetricHistory, name: string): HistoryChart {
  const days = h.days;
  const values = days.map((d) => d.value);
  const band: BandSeries = { lo: days.map((d) => d.low), hi: days.map((d) => d.high) };
  const markers = days.map((d): ChartMarker | null => (d.state === 'above' || d.state === 'below' ? { shape: 'dot', color: 'var(--warn)' } : null));
  const last = days.length - 1;
  const idx = last <= 0 ? [0] : [0, Math.round(last / 3), Math.round((last * 2) / 3), last];
  const xLabels = [...new Set(idx)].map((i) => ({ value: i, text: fmtMonthDay(days[i]!.date) }));
  const measured = values.filter((v) => v !== null).length;
  const outside = days.filter((d) => d.state === 'above' || d.state === 'below').length;
  const first = days[0]?.date;
  const end = days[last]?.date;
  const summary =
    measured === 0 || !first || !end
      ? `${name}: no readings in this period.`
      : `${name} over ${measured} ${measured === 1 ? 'night' : 'nights'} from ${fmtMonthDay(first)} to ${fmtMonthDay(end)}, ${outside === 0 ? 'none outside' : `${outside} outside`} the usual range.`;
  return { x: days.map((_, i) => i), values, band, markers, xLabels, summary, measured, hasBand: days.some((d) => d.low !== null) };
}

/** The short word of the Today card and the state it is coloured by. */
export function glanceWord(r: Pick<HealthStatusResponse, 'has_night' | 'outside' | 'compared' | 'metrics'>): { text: string; state: HealthState } {
  if (!r.has_night) return { text: 'No night yet', state: 'no_reading' };
  if (r.compared === 0) return r.metrics.some((m) => m.state === 'calibrating') ? { text: 'Calibrating', state: 'calibrating' } : { text: 'Not measured', state: 'no_reading' };
  if (r.outside === 0) return { text: 'In range', state: 'in_range' };
  return { text: `${r.outside} outside`, state: 'above' };
}

/** The first date of the 4-week chart that ends on `to` (28 nights). */
export const chartFrom = (to: string): string => addDays(to, -(WINDOW_NIGHTS - 1));
