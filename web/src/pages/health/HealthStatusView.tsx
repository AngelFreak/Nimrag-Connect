import type { HealthMetricHistory } from '../../api/generated/HealthMetricHistory';
import type { HealthMetricStatus } from '../../api/generated/HealthMetricStatus';
import type { HealthStatusResponse } from '../../api/generated/HealthStatusResponse';
import { Card, CardHeader, EmptyState, PageHeader, ProgressBar, SampleDataChip, SeriesChart, StatusWord, TopBar } from '../../components';
import { fmtShortDay } from '../../lib/dates';
import { useIsDesktop, usePageTitle } from '../../lib/hooks';
import { CardSkeleton, LoadError } from './parts';
import {
  METRIC_NAME,
  METRIC_ORDER,
  MIN_NIGHTS,
  NO_READINGS_YET,
  STATE_WORD,
  chartFrom,
  fmtMetric,
  historyChart,
  metricSentence,
  rangeText,
  stripLayout,
  summaryLine,
  summarySub,
  withoutNeverMeasured,
  type HistoryChart,
} from './healthStatus';
import { useHealthStatus, useHealthStatusHistory } from './healthStatusQueries';
import './healthStatus.css';

// Health status (/health/status): last night's HRV, lowest heart rate, breathing rate and blood
// oxygen against the person's own usual range from the 28 nights before. A comparison with their
// own habits, not a medical assessment, and the page says so.

const NOTE = 'The charts show the last 4 weeks: the shaded band is your usual range on each night, and orange dots are nights outside it. This compares last night with your own usual values from the 28 nights before. It is not a medical assessment. If you feel unwell, talk to a doctor, whatever the numbers say.';

export function HealthStatusView() {
  usePageTitle('Health status');
  const desktop = useIsDesktop();
  const q = useHealthStatus();
  const date = q.data?.date ?? null;
  const history = useHealthStatusHistory(date ? chartFrom(date) : null, date);
  const sample = q.data?.sample;
  return (
    <>
      {desktop ? (
        <PageHeader title="Health status" actions={<SampleDataChip sample={sample} />}>
          <p className="stats-note">Last night against your own usual range</p>
        </PageHeader>
      ) : (
        <TopBar variant="back" backTo="/health" backLabel="Back to Health" title="Health status" subtitle={date ? fmtShortDay(date) : null} aside={<SampleDataChip sample={sample} />} />
      )}
      <div className="page" style={{ paddingTop: 0 }} aria-busy={q.isPending}>
        {q.isPending ? (
          <>
            <CardSkeleton lines={2} height={28} />
            <CardSkeleton lines={4} height={20} />
          </>
        ) : q.isError && !q.data ? (
          <LoadError what="your health status" error={q.error} onRetry={() => void q.refetch()} />
        ) : q.data ? (
          <Body data={q.data} history={history.data?.metrics ?? null} historyFailed={history.isError && !history.data} />
        ) : null}
      </div>
    </>
  );
}

function Body({ data: all, history, historyFailed }: { data: HealthStatusResponse; history: HealthMetricHistory[] | null; historyFailed: boolean }) {
  const desktop = useIsDesktop();
  // A value that has never been measured is not shown at all (not even as "No reading").
  const data = withoutNeverMeasured(all);
  const metrics = METRIC_ORDER.map((k) => data.metrics.find((m) => m.key === k)).filter((m): m is HealthMetricStatus => m !== undefined);
  return (
    <div className="page-stack">
      <Summary data={data} />
      {data.has_night && metrics.length === 0 ? (
        <EmptyState title={NO_READINGS_YET}>Values appear here once the watch has measured them overnight.</EmptyState>
      ) : data.has_night ? (
        <div className={desktop ? 'grid-panels' : 'page-stack'}>
          {metrics.map((m) => (
            <MetricCard key={m.key} m={m} history={history?.find((h) => h.key === m.key) ?? null} historyFailed={historyFailed} />
          ))}
        </div>
      ) : (
        <EmptyState title="Nothing to compare yet">Wear the watch to sleep and sync it. The values appear here the next morning.</EmptyState>
      )}
      <p className="stats-note">{NOTE}</p>
    </div>
  );
}

function Summary({ data }: { data: HealthStatusResponse }) {
  const tone = data.has_night && data.compared > 0 ? (data.outside === 0 ? 'in_range' : 'above') : 'calibrating';
  return (
    <Card padding="lg">
      <p className="hstatus-summary">
        <StatusWord type="health" value={tone} size="lg">
          {summaryLine(data)}
        </StatusWord>
      </p>
      <p className="hstatus-sub">
        {data.has_night ? `Night ending ${fmtShortDay(data.date)}. ` : ''}
        {summarySub(data)}
      </p>
    </Card>
  );
}

/** The usual range as a band and the value as a dot on it. */
export function UsualRange({ m }: { m: HealthMetricStatus }) {
  const layout = stripLayout(m);
  if (!layout) return null;
  const pct = (n: number) => `${(Math.min(1, Math.max(0, n)) * 100).toFixed(2)}%`;
  return (
    <div className="usual" role="img" aria-label={metricSentence(m)}>
      <div className="usual__band" style={{ left: pct(layout.lo), width: `calc(${pct(layout.hi)} - ${pct(layout.lo)})` }} />
      <div className={`usual__dot usual__dot--${m.state}`} style={{ left: pct(layout.at) }} />
    </div>
  );
}

function MetricCard({ m, history, historyFailed }: { m: HealthMetricStatus; history: HealthMetricHistory | null; historyFailed: boolean }) {
  const desktop = useIsDesktop();
  const name = METRIC_NAME[m.key];
  const value = fmtMetric(m.value, m.unit);
  const range = rangeText(m);
  const chart = history ? historyChart(history, name) : null;
  return (
    <Card padding="lg" aria-label={name} data-metric={m.key}>
      <CardHeader title={name} tone="muted" aside={<StatusWord type="health" value={m.state}>{STATE_WORD[m.state]}</StatusWord>} />
      <div className="hstatus-row">
        {value ? <span className="hstatus-value">{value}</span> : null}
        <span className="hstatus-range">{range ? `Usual ${range}` : m.state === 'no_reading' ? 'Not measured last night' : null}</span>
      </div>
      {m.state === 'calibrating' ? (
        <div className="hstatus-calib">
          <ProgressBar value={Math.min(MIN_NIGHTS, m.nights)} max={MIN_NIGHTS} label={`${m.nights} of ${MIN_NIGHTS} nights collected`} color="var(--status-none)" height={8} />
          <p className="hstatus-sub">
            {m.nights} of {MIN_NIGHTS} nights so far. {m.nights_needed} more {m.nights_needed === 1 ? 'night' : 'nights'} needed before a usual range is shown.
          </p>
        </div>
      ) : (
        <UsualRange m={m} />
      )}
      <HistoryPlot chart={chart} failed={historyFailed} unit={m.unit} height={desktop ? 150 : 100} />
    </Card>
  );
}

function HistoryPlot({ chart, failed, unit, height }: { chart: HistoryChart | null; failed: boolean; unit: string; height: number }) {
  if (!chart) return failed ? <p className="hstatus-sub">The 4-week chart could not be loaded.</p> : <div style={{ height }} aria-hidden="true" />;
  if (chart.measured === 0) return <p className="hstatus-sub">No readings in the last 4 weeks.</p>;
  return (
    <div className="hstatus-chart">
      <SeriesChart
        label="Last 4 weeks"
        summary={chart.summary}
        x={chart.x}
        series={[{ label: 'Night', color: 'link', data: chart.values, width: 2, points: true, markers: chart.markers }]}
        {...(chart.hasBand ? { band: chart.band } : {})}
        xLabels={chart.xLabels}
        yUnit={unit === '%' || unit === 'brpm' ? 'decimal' : 'int'}
        height={height}
      />
    </div>
  );
}
