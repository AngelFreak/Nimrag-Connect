import { useNavigate } from 'react-router-dom';
import type { SleepBucket } from '../../api/generated/SleepBucket';
import type { SleepHistory } from '../../api/generated/SleepHistory';
import { BarSeries, Card, CardHeader, EmptyState, Legend, SampleDataChip, SeriesChart, StatTile, Table, TopBar } from '../../components';
import { fmtInt } from '../../lib/format';
import { useIsDesktop, usePageTitle } from '../../lib/hooks';
import { fmtSeconds } from './model';
import { CardSkeleton, ChartSummary, LoadError } from './parts';
import { PeriodNav, RangeBar, usePeriod, type PeriodOption } from './period';
import { useSleepHistory, type SleepGroup } from './queries';
import { bucketName, durationChartSummary, scoreChartSummary, sleepChart, sleepStatLabels, sleepSummary } from './ranges';

// The Sleep score page for more than one night: the score as a line, the time asleep as bars (green
// from 8 h), and the average, highest and lowest score. 7d and 4w show one point per night, 1y one
// per month. The one-night view (SleepView) is the "1d" option of the same control.

export const SLEEP_PERIODS = [
  { value: '1d', label: '1d', days: 1 },
  { value: '7d', label: '7d', days: 7 },
  { value: '4w', label: '4w', days: 28 },
  { value: '1y', label: '1y', days: 365 },
] as const satisfies readonly PeriodOption<string>[];
export type SleepPeriod = (typeof SLEEP_PERIODS)[number]['value'];

/** The segmented control of the Sleep page; picking 1d goes to the latest night. */
export function SleepRangeBar({ value }: { value: SleepPeriod }) {
  const navigate = useNavigate();
  return <RangeBar label="Period" options={SLEEP_PERIODS} value={value} onChange={(k) => void navigate(k === '1d' ? '/sleep' : `/sleep?range=${k}`)} />;
}

const GROUP: Record<SleepPeriod, SleepGroup> = { '1d': 'day', '7d': 'day', '4w': 'day', '1y': 'month' };

export function SleepRangeView() {
  usePageTitle('Sleep score');
  const period = usePeriod<SleepPeriod>(SLEEP_PERIODS, '7d');
  const q = useSleepHistory(period.from, period.to, GROUP[period.option.value]);
  const navigate = useNavigate();
  return (
    <>
      <TopBar variant="back" backTo="/health" backLabel="Back to Health" title="Sleep score" aside={<SampleDataChip sample={q.data?.sample} />} />
      <RangeBar label="Period" options={SLEEP_PERIODS} value={period.option.value} onChange={(k) => (k === '1d' ? void navigate('/sleep') : period.setRange(k))} />
      <div className="page" style={{ paddingTop: 0 }} aria-busy={q.isPending}>
        <PeriodNav from={period.from} to={period.to} atToday={period.atToday} onStep={period.step} unit="period" />
        {q.isPending ? (
          <>
            <CardSkeleton lines={4} height={20} />
            <CardSkeleton lines={3} height={20} />
          </>
        ) : q.isError && !q.data ? (
          <LoadError what="the sleep history" error={q.error} onRetry={() => void q.refetch()} />
        ) : q.data ? (
          <div className="page-stack" style={{ opacity: q.isPlaceholderData ? 0.7 : 1 }}>
            <Body data={q.data} />
          </div>
        ) : null}
      </div>
    </>
  );
}

function Body({ data }: { data: SleepHistory }) {
  if (data.stats.nights === 0) {
    return <EmptyState title="No sleep recorded in this period">Nights appear here after the watch has synced them. Try another period.</EmptyState>;
  }
  return (
    <>
      <div className="grid-panels" style={{ alignItems: 'start' }}>
        <ScoreCard data={data} />
        <DurationCard data={data} />
      </div>
      <DataTable data={data} />
    </>
  );
}

function ScoreCard({ data }: { data: SleepHistory }) {
  const desktop = useIsDesktop();
  const chart = sleepChart(data);
  const names = sleepStatLabels(data.group);
  const s = data.stats;
  const summary = scoreChartSummary(data);
  const high = data.group === 'day' ? s.highest_score : s.avg_highest;
  const low = data.group === 'day' ? s.lowest_score : s.avg_lowest;
  const slots = data.buckets.length;
  return (
    <Card padding="lg">
      <CardHeader title="Sleep score" tone="muted" aside={s.scored_nights > 0 ? `${s.scored_nights} scored night${s.scored_nights === 1 ? '' : 's'}` : undefined} />
      <div className="stat-trio">
        <StatTile size="sm" label={names.avg} value={fmtInt(s.avg_score)} tone="sleep" />
        <StatTile size="sm" label={names.high} value={fmtInt(high)} />
        <StatTile size="sm" label={names.low} value={fmtInt(low)} />
      </div>
      <SeriesChart
        label={`Sleep score ${data.group === 'month' ? 'by month' : 'by night'}`}
        summary={summary}
        x={chart.x}
        series={[{ label: 'Sleep score', color: 'sleep', data: chart.score, points: true, width: 2 }]}
        xRange={[-0.5, slots - 0.5]}
        height={desktop ? 170 : 130}
        xLabels={chart.labels && chart.labels.length <= 13 ? chart.labels.map((text, value) => ({ value, text })) : chart.axisLabels}
      />
      <ChartSummary hidden={chart.score.some((v) => v !== null)}>{summary}</ChartSummary>
    </Card>
  );
}

function DurationCard({ data }: { data: SleepHistory }) {
  const chart = sleepChart(data);
  const goalMin = data.long_night_s / 60;
  const goalText = `${Math.round(data.long_night_s / 3600)} h`;
  const per = data.group === 'month' ? 'month' : 'night';
  return (
    <Card padding="lg">
      <CardHeader title="Sleep duration" tone="muted" aside={fmtSeconds(data.stats.avg_duration_s) ? `Avg ${fmtSeconds(data.stats.avg_duration_s)}` : undefined} />
      <BarSeries
        values={chart.minutes}
        labels={chart.labels ?? chart.minutes.map(() => '')}
        {...(chart.labels ? {} : { axisLabels: chart.axis })}
        label={`Time asleep per ${per}`}
        yUnit="minutes"
        color="var(--good)"
        belowGoalColor="var(--accent)"
        goal={goalMin}
        summary={durationChartSummary(data)}
        summaryVisible
        height={110}
      />
      <Legend
        items={[
          { key: 'under', label: `Under ${goalText}`, color: 'var(--accent)' },
          { key: 'over', label: `${goalText} or more`, color: 'var(--good)' },
        ]}
      />
    </Card>
  );
}

/** The same numbers as a table, for those who prefer to read them. */
function DataTable({ data }: { data: SleepHistory }) {
  const rows = data.buckets.filter((b) => b.nights > 0);
  return (
    <details className="data-details">
      <summary>Show the numbers as a table</summary>
      <p className="stat__sub">{sleepSummary(data)}</p>
      <Table<SleepBucket>
        caption={data.group === 'month' ? 'Sleep by month' : 'Sleep by night'}
        density="compact"
        rows={rows}
        rowKey={(b) => b.from}
        columns={[
          { key: 'when', header: data.group === 'month' ? 'Month' : 'Night', cell: (b) => bucketName(b, data.group) },
          { key: 'nights', header: 'Nights', cell: (b) => String(b.nights), align: 'end' },
          { key: 'score', header: 'Score', cell: (b) => fmtInt(b.avg_score), align: 'end' },
          { key: 'asleep', header: 'Time asleep', cell: (b) => fmtSeconds(b.avg_duration_s), align: 'end' },
        ]}
      />
    </details>
  );
}
