import { useState } from 'react';
import { Link } from 'react-router-dom';
import type { HrvDay } from '../../api/generated/HrvDay';
import type { HrvRange } from '../../api/generated/HrvRange';
import type { HrvStatusKind } from '../../api/generated/HrvStatusKind';
import { Card, CardHeader, DataList, EmptyState, Legend, SampleDataChip, SeriesChart, StatTile, TopBar, Toggle, ValueOrUnavailable, type LegendItem } from '../../components';
import { fmtDayLong, fmtMonthDay } from '../../lib/dates';
import { fmtInt, statusName, withUnit } from '../../lib/format';
import { useIsDesktop, usePageTitle } from '../../lib/hooks';
import { CardSkeleton, ChartSummary, LoadError } from './parts';
import { PeriodNav, RangeBar, usePeriod, type PeriodOption } from './period';
import { useHrv } from './queries';
import { baselineShort, baselineText, dayAxisLabels, hrvChart, hrvChartSummary, hrvHasData, hrvValue, type HrvMode } from './ranges';
import { clock } from './time';

// The HRV status page. 7d and 4w: the 7-day averages against the balanced range, with a marker per
// day for its status (green dot Balanced, orange square Unbalanced, red triangle Low or Poor), the
// average of the overnight values, and a card per day. 1d: that night's readings. Everything comes
// from GET /api/hrv.

const OPTIONS = [
  { value: '1d', label: '1d', days: 1 },
  { value: '7d', label: '7d', days: 7 },
  { value: '4w', label: '4w', days: 28 },
] as const satisfies readonly PeriodOption<string>[];
type Key = (typeof OPTIONS)[number]['value'];

const MARK_COLOR: Record<HrvStatusKind, string> = {
  balanced: 'var(--hrv-balanced)',
  unbalanced: 'var(--hrv-unbalanced)',
  low: 'var(--hrv-low)',
  poor: 'var(--hrv-low)',
};

/** The shape and colour of a status, as drawn in the chart; `null` (no status) is a grey dot. */
export function StatusMark({ status }: { status: HrvStatusKind | null }) {
  const color = status ? MARK_COLOR[status] : 'var(--text-2)';
  return (
    <svg className="mark" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true" focusable="false" style={{ fill: color }}>
      {status === 'unbalanced' ? (
        <rect x="1.5" y="1.5" width="9" height="9" />
      ) : status === 'low' || status === 'poor' ? (
        <polygon points="6,0.5 11.5,10.5 0.5,10.5" />
      ) : (
        <circle cx="6" cy="6" r="5" />
      )}
    </svg>
  );
}

export function HrvView() {
  usePageTitle('HRV status');
  const period = usePeriod<Key>(OPTIONS, '7d');
  const single = period.option.value === '1d';
  const q = useHrv(period.from, period.to);
  return (
    <>
      <TopBar variant="back" backTo="/health" backLabel="Back to Health" title="HRV status" aside={<SampleDataChip sample={q.data?.sample} />} />
      <RangeBar label="Period" options={OPTIONS} value={period.option.value} onChange={period.setRange} />
      <div className="page" style={{ paddingTop: 0 }} aria-busy={q.isPending}>
        <PeriodNav from={period.from} to={period.to} atToday={period.atToday} onStep={period.step} unit={single ? 'day' : 'period'} />
        {q.isPending ? (
          <>
            <CardSkeleton lines={4} height={20} />
            <CardSkeleton lines={2} height={20} />
          </>
        ) : q.isError && !q.data ? (
          <LoadError what="the HRV status" error={q.error} onRetry={() => void q.refetch()} />
        ) : q.data ? (
          <div className="page-stack" style={{ opacity: q.isPlaceholderData ? 0.7 : 1 }}>
            {single ? <NightBody data={q.data} /> : <RangeBody data={q.data} />}
          </div>
        ) : null}
      </div>
    </>
  );
}

// ---- 7d and 4w --------------------------------------------------------------------------------------

function RangeBody({ data }: { data: HrvRange }) {
  const [mode, setMode] = useState<HrvMode>('weekly');
  const withData = data.days.filter(hrvHasData);
  const weekly = [...data.days].reverse().find((d) => d.weekly_avg_ms !== null)?.weekly_avg_ms ?? null;
  const range = baselineShort(data.baseline_low_ms, data.baseline_high_ms);
  return (
    <>
      <div className="hrv-top">
        <AveragesCard data={data} mode={mode} onMode={setMode} />
        <div className="stat-trio">
          <StatTile
            size="sm"
            label="Overnight avg"
            value={withUnit(fmtInt(data.overnight_avg_ms), 'ms')}
            sub={data.nights > 0 ? `${data.nights} night${data.nights === 1 ? '' : 's'}` : undefined}
          />
          <StatTile size="sm" label="Baseline" value={withUnit(range, 'ms')} />
          <StatTile size="sm" label="7-day avg" value={withUnit(fmtInt(weekly), 'ms')} sub={weekly === null ? undefined : 'latest'} />
        </div>
      </div>
      <section aria-labelledby="hrv-days" className="page-stack">
        <h2 className="section-label" id="hrv-days">
          Day by day
        </h2>
        {withData.length === 0 ? (
          <EmptyState title="No HRV in this period">The watch records HRV overnight. Days appear here after a night with a reading.</EmptyState>
        ) : (
          <ul className="day-cards">
            {[...withData].reverse().map((d) => (
              <DayCard key={d.date} day={d} />
            ))}
          </ul>
        )}
      </section>
    </>
  );
}

function AveragesCard({ data, mode, onMode }: { data: HrvRange; mode: HrvMode; onMode: (m: HrvMode) => void }) {
  const desktop = useIsDesktop();
  const chart = hrvChart(data.days, mode);
  const summary = hrvChartSummary(data, mode);
  const items: LegendItem[] = [
    { key: 'balanced', label: 'Balanced', color: 'var(--hrv-balanced)', shape: 'dot' },
    { key: 'unbalanced', label: 'Unbalanced', color: 'var(--hrv-unbalanced)', shape: 'square' },
    { key: 'low', label: 'Low or poor', color: 'var(--hrv-low)', shape: 'triangle' },
    ...(chart.band ? [{ key: 'band', label: 'Balanced range', color: 'var(--band)', outlined: true }] : []),
  ];
  return (
    <Card padding="lg">
      <CardHeader title={mode === 'weekly' ? '7-day averages' : 'Overnight averages'} tone="muted" />
      <Toggle label="Overnight averages" checked={mode === 'overnight'} onChange={(e) => onMode(e.target.checked ? 'overnight' : 'weekly')} />
      <SeriesChart
        label={mode === 'weekly' ? '7-day average HRV by day' : 'Overnight HRV by day'}
        summary={summary}
        x={chart.x}
        series={[{ label: mode === 'weekly' ? '7-day average' : 'Overnight average', color: 'link', data: chart.y, markers: chart.markers, width: 1.5 }]}
        {...(chart.band ? { band: chart.band } : {})}
        xRange={[-0.5, data.days.length - 0.5]}
        height={desktop ? 170 : 130}
        xLabels={dayAxisLabels(data.from, data.days.length)}
      />
      <ChartSummary hidden={chart.count > 0}>{summary}</ChartSummary>
      <Legend items={items} />
    </Card>
  );
}

function DayCard({ day }: { day: HrvDay }) {
  return (
    <Card as="li" padding="lg" className="day-card">
      <div className="day-card__head">
        <Link className="day-card__date" to={`/hrv?range=1d&end=${day.date}`}>
          {fmtDayLong(day.date)}
        </Link>
        <span className="day-card__status">
          <StatusMark status={day.status} />
          {statusName(day.status_label) ?? 'No status'}
        </span>
      </div>
      <dl className="day-card__vals">
        <div>
          <dt>7-day avg</dt>
          <dd>
            <ValueOrUnavailable value={withUnit(fmtInt(day.weekly_avg_ms), 'ms')} />
          </dd>
        </div>
        <div>
          <dt>Overnight avg</dt>
          <dd>
            <ValueOrUnavailable value={withUnit(fmtInt(day.last_night_ms), 'ms')} />
          </dd>
        </div>
      </dl>
    </Card>
  );
}

// ---- 1d ---------------------------------------------------------------------------------------------

function NightBody({ data }: { data: HrvRange }) {
  const day = data.days[0];
  if (!day || (!hrvHasData(day) && !data.night)) {
    return <EmptyState title="No HRV for this night">The watch records HRV overnight. Try the previous day.</EmptyState>;
  }
  return (
    <div className="grid-panels" style={{ alignItems: 'start' }}>
      <Card padding="xl">
        <CardHeader title={`Overnight HRV, ${fmtMonthDay(day.date)}`} tone="muted" />
        <div className="status-hero">
          <StatusMark status={day.status} />
          <span className="stat__value" style={{ fontSize: 34 }}>
            {withUnit(fmtInt(hrvValue(day, 'overnight')), 'ms') ?? <span className="unavailable">unavailable</span>}
          </span>
        </div>
        <span className="stat__sub">{statusName(day.status_label) ?? 'No status for this night'}</span>
        <DataList
          items={[
            { label: '7-day average', value: withUnit(fmtInt(day.weekly_avg_ms), 'ms') },
            { label: 'Highest 5-minute average', value: withUnit(fmtInt(day.last_night_5min_high_ms), 'ms') },
            { label: 'Balanced range', value: baselineText(day.baseline_low_ms, day.baseline_high_ms) },
          ]}
        />
      </Card>
      <ReadingsCard day={day} night={data.night} />
    </div>
  );
}

function ReadingsCard({ day, night }: { day: HrvDay; night: HrvRange['night'] }) {
  const desktop = useIsDesktop();
  if (!night) {
    return (
      <Card padding="lg">
        <CardHeader title="Readings through the night" tone="muted" />
        <p className="stat__sub">The individual readings were not recorded for this night.</p>
      </Card>
    );
  }
  const secs = night.readings.map((r) => Date.parse(r.time) / 1000);
  const band: [number, number] | undefined = day.baseline_low_ms !== null && day.baseline_high_ms !== null ? [day.baseline_low_ms, day.baseline_high_ms] : undefined;
  const first = night.readings[0]?.time;
  const last = night.readings[night.readings.length - 1]?.time;
  const summary = `${night.readings.length} HRV reading${night.readings.length === 1 ? '' : 's'} through the night, from ${fmtInt(night.low_ms)} to ${fmtInt(night.high_ms)} ms, average ${fmtInt(night.avg_ms)} ms.`;
  return (
    <Card padding="lg">
      <CardHeader title="Readings through the night" tone="muted" aside={withUnit(fmtInt(night.avg_ms), 'ms average') ?? undefined} />
      <SeriesChart
        label="HRV readings through the night"
        summary={summary}
        x={secs}
        series={[{ label: 'HRV', color: 'link', data: night.readings.map((r) => r.value), points: night.readings.length <= 12 }]}
        {...(band ? { band } : {})}
        height={desktop ? 170 : 120}
        xLabels={secs.length < 2 ? [] : [{ value: secs[0] as number, text: clock(first) ?? '' }, { value: secs[secs.length - 1] as number, text: clock(last) ?? '' }]}
      />
      <ChartSummary hidden>{summary}</ChartSummary>
      <div className="stat-trio">
        <StatTile size="sm" label="Lowest" value={withUnit(fmtInt(night.low_ms), 'ms')} />
        <StatTile size="sm" label="Average" value={withUnit(fmtInt(night.avg_ms), 'ms')} />
        <StatTile size="sm" label="Highest" value={withUnit(fmtInt(night.high_ms), 'ms')} />
      </div>
      {band ? <Legend items={[{ key: 'band', label: 'Balanced range', color: 'var(--band)', outlined: true }]} /> : null}
    </Card>
  );
}
