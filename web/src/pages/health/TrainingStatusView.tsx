import type { CSSProperties } from 'react';
import type { TrainingStatusRange } from '../../api/generated/TrainingStatusRange';
import { Card, CardHeader, DataList, EmptyState, Legend, SampleDataChip, Table, TopBar, Unavailable, type LegendItem } from '../../components';
import { fmtMonthDay } from '../../lib/dates';
import { fmtFixed, fmtInt } from '../../lib/format';
import { usePageTitle } from '../../lib/hooks';
import { CardSkeleton, LoadError } from './parts';
import { PeriodNav, RangeBar, usePeriod, type PeriodOption } from './period';
import { useTrainingStatus } from './queries';
import { STATUS_COLOR, runDates, timelineSummary } from './ranges';

// The Training status page: the status day by day as a coloured timeline, one row per run of equal
// status, and (on "Current") today's status with the load figures. Names and kinds of the statuses
// come from GET /api/training/status; this file never names a status code.

const OPTIONS = [
  { value: 'current', label: 'Current', days: 7 },
  { value: '4w', label: '4w', days: 28 },
  { value: '12w', label: '12w', days: 84 },
  { value: '6m', label: '6m', days: 182 },
] as const satisfies readonly PeriodOption<string>[];
type Key = (typeof OPTIONS)[number]['value'];

export function TrainingStatusView() {
  usePageTitle('Training status');
  const period = usePeriod<Key>(OPTIONS, 'current');
  const q = useTrainingStatus(period.from, period.to);
  const unit = period.option.value === 'current' ? 'week' : 'period';
  return (
    <>
      <TopBar variant="back" backTo="/training" backLabel="Back to Training" title="Training status" aside={<SampleDataChip sample={q.data?.sample} />} />
      <RangeBar label="Period" options={OPTIONS} value={period.option.value} onChange={period.setRange} />
      <div className="page" style={{ paddingTop: 0 }} aria-busy={q.isPending}>
        <PeriodNav from={period.from} to={period.to} atToday={period.atToday} onStep={period.step} unit={unit} />
        {q.isPending ? (
          <>
            <CardSkeleton lines={2} height={28} />
            <CardSkeleton lines={3} height={20} />
          </>
        ) : q.isError && !q.data ? (
          <LoadError what="the training status" error={q.error} onRetry={() => void q.refetch()} />
        ) : q.data ? (
          <div className="page-stack" style={{ opacity: q.isPlaceholderData ? 0.7 : 1 }}>
            <Body data={q.data} current={period.option.value === 'current'} />
          </div>
        ) : null}
      </div>
    </>
  );
}

function Body({ data, current }: { data: TrainingStatusRange; current: boolean }) {
  return (
    <>
      {current ? <CurrentCard data={data} /> : null}
      <div className="grid-panels" style={{ alignItems: 'start' }}>
        <TimelineCard data={data} />
        <HistoryCard data={data} />
      </div>
    </>
  );
}

function CurrentCard({ data }: { data: TrainingStatusRange }) {
  const { current, load } = data;
  const asOf = current.as_of ? current.as_of.slice(0, 10) : null;
  const since = current.since ? `Since ${fmtMonthDay(current.since)}${current.since_is_lower_bound ? ' or earlier' : ''}` : null;
  const kind = current.kind ?? 'none';
  return (
    <Card padding="xl">
      <CardHeader title="Training status now" tone="muted" aside={asOf && asOf < data.today ? `As of ${fmtMonthDay(asOf)}` : undefined} />
      <div className="status-now">
        <div className="stat" style={{ gap: 8 }}>
          {current.label ? (
            <>
              <div className="status-hero">
                <span className="status-hero__dot" aria-hidden="true" style={{ background: STATUS_COLOR[kind] }} />
                <span className="stat__value" style={{ fontSize: 34 }}>
                  {current.label}
                </span>
              </div>
              {since ? <span className="stat__sub">{since}</span> : null}
            </>
          ) : (
            <>
              <span className="stat__value" style={{ fontSize: 34 }}>
                <Unavailable />
              </span>
              <span className="stat__sub">No training status has been recorded yet.</span>
            </>
          )}
        </div>
        <DataList
          items={[
            { label: 'Acute load', value: fmtInt(load.acute) },
            { label: 'Chronic load', value: fmtInt(load.chronic) },
            { label: 'Load ratio', value: fmtFixed(load.ratio, 2) },
          ]}
        />
      </div>
    </Card>
  );
}

/** One segment per day; a day without a reading is grey like "No status", and the text says which. */
function StatusTimeline({ data }: { data: TrainingStatusRange }) {
  const dense = data.days.length > 31;
  return (
    <div className={dense ? 'status-timeline status-timeline--dense' : 'status-timeline'} role="img" aria-label={timelineSummary(data)}>
      {data.days.map((d) => (
        <span key={d.date} className="status-timeline__day" style={{ background: STATUS_COLOR[d.status?.kind ?? 'none'] }} />
      ))}
    </div>
  );
}

function TimelineCard({ data }: { data: TrainingStatusRange }) {
  // The colour key from the server, plus every unnamed code that shows up in the period (grey).
  const unnamed = [...new Set(data.days.flatMap((d) => (d.status?.kind === 'unknown' ? [d.status.label] : [])))];
  const items: LegendItem[] = [
    ...data.legend.map((l) => ({ key: l.label, label: l.label, color: STATUS_COLOR[l.kind] })),
    ...unnamed.map((label) => ({ key: label, label, color: STATUS_COLOR.unknown })),
  ];
  return (
    <Card padding="lg">
      <CardHeader title="Status by day" tone="muted" />
      <StatusTimeline data={data} />
      <div className="chart__axis" aria-hidden="true">
        <span>{fmtMonthDay(data.from)}</span>
        <span>{fmtMonthDay(data.to)}</span>
      </div>
      <Legend items={items} />
    </Card>
  );
}

function HistoryCard({ data }: { data: TrainingStatusRange }) {
  return (
    <Card padding="lg">
      <CardHeader title="Status history" tone="muted" />
      {data.runs.length === 0 ? (
        <EmptyState title="No training status in this period">The watch has not reported one for these days. Days without a reading are grey in the timeline.</EmptyState>
      ) : (
        <Table
          caption="Training status by date, newest first"
          rows={data.runs}
          rowKey={(r) => `${r.status.label}-${r.from}`}
          columns={[
            {
              key: 'status',
              header: 'Status',
              cell: (r) => (
                <span className="status-cell">
                  <span className="swatch" aria-hidden="true" style={{ '--swatch': STATUS_COLOR[r.status.kind] } as CSSProperties} />
                  {r.status.label}
                </span>
              ),
            },
            { key: 'dates', header: 'Dates', cell: (r) => runDates(r), align: 'end' },
          ]}
        />
      )}
    </Card>
  );
}
