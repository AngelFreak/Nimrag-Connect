import { useQuery } from '@tanstack/react-query';
import { api } from '../api/client';
import type { TrainingResponse } from '../api/generated';
import { BarList, Card, CardHeader, DataList, PageHeader, RangeBar, Ring, SampleDataChip, SeriesChart, StatStrip, TopBar, EmptyNote } from '../components';
import { Link } from 'react-router-dom';
import { fmtShortDay } from '../lib/dates';
import { fmtFixed, fmtInt } from '../lib/format';
import { useIsDesktop, usePageTitle } from '../lib/hooks';
import { ScoreHero, StatsColumns, StatsSection } from './health/desktop';
import { ReadinessCard } from './health/ReadinessCard';
import { RaceCard } from './race/RaceCard';
import { StatusExplanation } from './health/StatusExplanation';
import { PageError, PageSkeleton } from './body/common';
import { fmtLocalStamp, localParts } from './body/time';
import {
  fmtRecoveryLeft,
  seriesSummary,
  seriesValues,
  sinceText,
  statusView,
  targetText,
  vsTargetText,
} from './body/training';

// Stage 3j: GET /api/training. VO2 max is the watch's maximal MET x 3.5 and says so.

export function TrainingPage() {
  usePageTitle('Training');
  const query = useQuery({
    queryKey: ['training'],
    queryFn: ({ signal }) => api.get<TrainingResponse>('/api/training', { signal }),
  });
  const data = query.data;
  const asOf = localParts(data?.as_of);
  const desktop = useIsDesktop();

  return (
    <>
      {desktop ? (
        <PageHeader title="Training" actions={<SampleDataChip sample={data?.sample} />}>
          <p className="stats-note">{asOf ? `As of ${fmtShortDay(asOf.day)}` : 'Recovery, load and fitness'}</p>
        </PageHeader>
      ) : (
        <TopBar
          variant="back"
          backTo="/health"
          title="Training"
          subtitle={asOf ? `As of ${fmtShortDay(asOf.day)}` : 'Recovery, load and fitness'}
          aside={<SampleDataChip sample={data?.sample} />}
        />
      )}
      {query.isError ? (
        <PageError title="Training could not be loaded" error={query.error} onRetry={() => void query.refetch()} />
      ) : !data ? (
        <PageSkeleton cards={4} />
      ) : (
        <TrainingBody data={data} />
      )}
    </>
  );
}

export function TrainingBody({ data }: { data: TrainingResponse }) {
  const desktop = useIsDesktop();
  if (desktop) {
    return (
      <div className="page" style={{ paddingTop: 0 }}>
        <TrainingDesktop data={data} />
      </div>
    );
  }
  return (
    <div className="page">
      <ReadinessCard />
      <RecoveryCard recovery={data.recovery} />
      <div className="grid-panels">
        <LoadCard load={data.load} />
        <BalanceCard balance={data.balance} />
        <StatusCard status={data.status} />
        <Vo2Card vo2={data.vo2max} />
      </div>
      <RaceCard />
    </div>
  );
}

function RecoveryCard({ recovery }: { recovery: TrainingResponse['recovery'] }) {
  const left = fmtRecoveryLeft(recovery.remaining_s);
  const done = recovery.remaining_s !== null && recovery.remaining_s <= 0;
  const readyAt = fmtLocalStamp(recovery.ready_at);
  const pct = recovery.percent_complete;
  return (
    <Card padding="lg" row>
      <div className="stat" style={{ gap: 4 }}>
        <h2 className="card__title card__title--muted">Recovery time left</h2>
        {done || left ? (
          <span className="stat__value" style={{ fontSize: 34 }}>
            {done ? 'Recovered' : left}
          </span>
        ) : (
          <EmptyNote>No recovery time from the watch yet.</EmptyNote>
        )}
        {done ? (
          <span className="stat__sub tone-link">Ready for a hard session</span>
        ) : readyAt ? (
          <span className="stat__sub tone-link">Fully recovered {readyAt}</span>
        ) : null}
      </div>
      {pct === null ? null : (
        <Ring
          value={pct}
          max={100}
          label="Recovery complete, percent"
          color="var(--accent)"
          size={72}
          strokeWidth={8}
          showValue={false}
        />
      )}
    </Card>
  );
}

function LoadCard({ load }: { load: TrainingResponse['load'] }) {
  const value = load.seven_day_load;
  const range = targetText(load.target_min, load.target_max);
  const where = vsTargetText(load.vs_target);
  const hasTarget = load.target_min !== null && load.target_max !== null;
  const axisMax = Math.max(load.target_max ?? 0, value ?? 0, 1) * 1.4;
  const valueText = fmtInt(value);
  const hasSeries = load.series_4w.some((v) => v !== null);
  return (
    <Card padding="lg">
      <CardHeader title="7-day load" {...(range ? { aside: `Target range ${range}` } : {})} />
      {valueText === null && !hasSeries ? (
        <EmptyNote>No training load from the watch in the last 4 weeks.</EmptyNote>
      ) : null}
      {valueText !== null ? (
        <span className="stat__value" style={{ fontSize: 28 }}>
          {valueText}
        </span>
      ) : null}
      {hasTarget && value !== null ? (
        <RangeBar
          value={value}
          min={0}
          max={axisMax}
          target={[load.target_min as number, load.target_max as number]}
          label={`Load of ${valueText} ${where ? where.toLowerCase() : 'against a target range'} of ${range}`}
        />
      ) : null}
      {where ? <span className="stat__sub">{where}</span> : null}
      {hasSeries ? (
        <SeriesChart
          label="Seven-day load over the last 4 weeks"
          summary={seriesSummary('Seven-day load', 'over the last 4 weeks', load.series_4w)}
          x={load.series_4w.map((_, i) => i)}
          series={[{ label: 'Load', color: 'link', data: seriesValues(load.series_4w) }]}
          height={88}
        />
      ) : null}
      <DataList
        items={[
          { label: 'Acute load', value: fmtInt(load.acute) },
          { label: 'Chronic load', value: fmtInt(load.chronic) },
          { label: 'Acute to chronic ratio', value: fmtFixed(load.ratio, 2) },
        ]}
      />
    </Card>
  );
}

function BalanceCard({ balance }: { balance: TrainingResponse['balance'] }) {
  if (balance === null) {
    return (
      <Card padding="lg">
        <CardHeader title="Load by intensity" />
        <EmptyNote>No load by intensity from the watch yet.</EmptyNote>
      </Card>
    );
  }
  const rows = [
    { key: 'low', label: 'Easy aerobic', value: balance.aerobic_low, min: balance.aerobic_low_target_min, max: balance.aerobic_low_target_max, color: 'var(--stress-rest)' },
    { key: 'high', label: 'Hard aerobic', value: balance.aerobic_high, min: balance.aerobic_high_target_min, max: balance.aerobic_high_target_max, color: 'var(--accent)' },
    { key: 'an', label: 'Anaerobic', value: balance.anaerobic, min: balance.anaerobic_target_min, max: balance.anaerobic_target_max, color: 'var(--warn)' },
  ];
  const top = Math.max(1, ...rows.map((r) => Math.max(r.value ?? 0, r.max ?? 0)));
  return (
    <Card padding="lg">
      <CardHeader title="Load by intensity" />
      <BarList
        label="Load by intensity"
        max={top}
        labelWidth={104}
        items={rows.map((r) => ({ key: r.key, label: r.label, value: r.value, color: r.color, text: fmtInt(r.value) ?? undefined }))}
      />
      <DataList
        items={rows.map((r) => ({
          label: `${r.label} target`,
          value: targetText(r.min, r.max),
        }))}
      />
    </Card>
  );
}

function StatusCard({ status }: { status: TrainingResponse['status'] }) {
  const view = statusView(status);
  const since = sinceText(status);
  return (
    <Card padding="lg">
      <CardHeader title="Training status" aside={<Link to="/training/status">History</Link>} />
      {view.headline ? (
        <span className="stat__value" style={{ fontSize: 28 }}>
          {view.headline}
        </span>
      ) : (
        <EmptyNote>No training status from the watch yet.</EmptyNote>
      )}
      {view.note ? <span className="stat__sub">{view.note}</span> : null}
      {since ? <span className="stat__sub">{since}</span> : null}
      <StatusExplanation brief />
    </Card>
  );
}

function Vo2Card({ vo2 }: { vo2: TrainingResponse['vo2max'] }) {
  const value = fmtFixed(vo2.value, 1);
  const change = vo2.change_3m === null ? null : `${vo2.change_3m > 0 ? '+' : ''}${fmtFixed(vo2.change_3m, 1)} in 3 months`;
  return (
    <Card padding="lg">
      <CardHeader title="VO2 max (derived)" {...(change ? { aside: <span className="tone-link">{change}</span> } : {})} />
      {value ? (
        <span className="stat__value" style={{ fontSize: 28 }}>
          {value}
        </span>
      ) : (
        <EmptyNote>No VO2 max estimate yet.</EmptyNote>
      )}
      <span className="stat__sub">derived (max MET × 3.5), ml/kg/min. This is not the watch's own VO2 max figure.</span>
      <SeriesChart
        label="VO2 max over the last 3 months"
        summary={seriesSummary('VO2 max', 'over the last 3 months', vo2.series_3m, 1)}
        x={vo2.series_3m.map((_, i) => i)}
        series={[{ label: 'VO2 max', color: 'linkSoft', data: seriesValues(vo2.series_3m) }]}
        yUnit="decimal"
        height={88}
      />
    </Card>
  );
}

// ---- wide screen ------------------------------------------------------------------------------------

function TrainingDesktop({ data }: { data: TrainingResponse }) {
  const { recovery, load, balance, status, vo2max: vo2 } = data;
  const left = fmtRecoveryLeft(recovery.remaining_s);
  const done = recovery.remaining_s !== null && recovery.remaining_s <= 0;
  const readyAt = fmtLocalStamp(recovery.ready_at);
  const view = statusView(status);
  const since = sinceText(status);
  const vo2Value = fmtFixed(vo2.value, 1);
  const vo2Change = vo2.change_3m === null ? null : `${vo2.change_3m > 0 ? '+' : ''}${fmtFixed(vo2.change_3m, 1)} in 3 months`;
  const loadText = fmtInt(load.seven_day_load);
  const range = targetText(load.target_min, load.target_max);
  const where = vsTargetText(load.vs_target);
  const hasTarget = load.target_min !== null && load.target_max !== null;
  const axisMax = Math.max(load.target_max ?? 0, load.seven_day_load ?? 0, 1) * 1.4;
  const rows =
    balance === null
      ? []
      : [
          { key: 'low', label: 'Easy aerobic', value: balance.aerobic_low, min: balance.aerobic_low_target_min, max: balance.aerobic_low_target_max, color: 'var(--stress-rest)' },
          { key: 'high', label: 'Hard aerobic', value: balance.aerobic_high, min: balance.aerobic_high_target_min, max: balance.aerobic_high_target_max, color: 'var(--accent)' },
          { key: 'an', label: 'Anaerobic', value: balance.anaerobic, min: balance.anaerobic_target_min, max: balance.anaerobic_target_max, color: 'var(--warn)' },
        ];
  const top = Math.max(1, ...rows.map((r) => Math.max(r.value ?? 0, r.max ?? 0)));
  return (
    <StatsColumns
      side={
        <>
          <StatsSection title="Readiness">
            <ReadinessCard flat />
          </StatsSection>
          <StatsSection title="Recovery Time">
            {done || left ? (
              <ScoreHero text value={done ? 'Recovered' : left} label={done ? 'Ready for a hard session' : readyAt ? `Fully recovered ${readyAt}` : 'Recovery time left'} />
            ) : (
              <EmptyNote>No recovery time from the watch yet.</EmptyNote>
            )}
            <div style={{ display: 'flex', justifyContent: 'center' }}>
              {recovery.percent_complete === null ? null : <Ring value={recovery.percent_complete} max={100} label="Recovery complete, percent" color="var(--accent)" size={96} strokeWidth={10} showValue={false} />}
            </div>
          </StatsSection>
          <StatsSection title="Training Status" aside={<Link to="/training/status">History</Link>}>
            {view.headline ? <ScoreHero text value={view.headline} {...(view.note || since ? { label: view.note ?? since } : {})} /> : <EmptyNote>No training status from the watch yet.</EmptyNote>}
            {view.note && since ? <p className="stats-note">{since}</p> : null}
            <StatusExplanation brief />
          </StatsSection>
          <StatsSection title="Race Predictor">
            <RaceCard flat />
          </StatsSection>
          <StatsSection title="VO2 Max (derived)">
            {vo2Value ? <ScoreHero value={vo2Value} {...(vo2Change ? { label: vo2Change } : {})} /> : <EmptyNote>No VO2 max estimate yet.</EmptyNote>}
            <p className="stats-note">Derived (max MET x 3.5), ml/kg/min. This is not the watch's own VO2 max figure.</p>
          </StatsSection>
        </>
      }
    >
      <StatsSection title="7-day Load" aside={range ? `Target range ${range}` : undefined}>
        {loadText === null && !load.series_4w.some((v) => v !== null) ? <EmptyNote>No training load from the watch in the last 4 weeks.</EmptyNote> : null}
        {loadText !== null ? <span className="big-value">{loadText}</span> : null}
        {hasTarget && load.seven_day_load !== null ? (
          <RangeBar
            value={load.seven_day_load}
            min={0}
            max={axisMax}
            target={[load.target_min as number, load.target_max as number]}
            label={`Load of ${loadText} ${where ? where.toLowerCase() : 'against a target range'} of ${range}`}
          />
        ) : null}
        {where ? <p className="stats-note">{where}</p> : null}
        {load.series_4w.some((v) => v !== null) ? (
          <SeriesChart
            label="Seven-day load over the last 4 weeks"
            summary={seriesSummary('Seven-day load', 'over the last 4 weeks', load.series_4w)}
            x={load.series_4w.map((_, i) => i)}
            series={[{ label: 'Load', color: 'link', data: seriesValues(load.series_4w) }]}
            height={200}
          />
        ) : null}
        <StatStrip
          items={[
            { key: 'acute', label: 'Acute load', value: fmtInt(load.acute) },
            { key: 'chronic', label: 'Chronic load', value: fmtInt(load.chronic) },
            { key: 'ratio', label: 'Acute to chronic ratio', value: fmtFixed(load.ratio, 2) },
          ]}
        />
      </StatsSection>

      <StatsSection title="Load by Intensity">
        {balance === null ? (
          <EmptyNote>No load by intensity from the watch yet.</EmptyNote>
        ) : (
          <>
            <BarList label="Load by intensity" max={top} labelWidth={104} items={rows.map((r) => ({ key: r.key, label: r.label, value: r.value, color: r.color, text: fmtInt(r.value) ?? undefined }))} />
            <StatStrip items={rows.map((r) => ({ key: r.key, label: `${r.label} target`, value: targetText(r.min, r.max) }))} />
          </>
        )}
      </StatsSection>

      <StatsSection title="VO2 Max">
        <SeriesChart
          label="VO2 max over the last 3 months"
          summary={seriesSummary('VO2 max', 'over the last 3 months', vo2.series_3m, 1)}
          x={vo2.series_3m.map((_, i) => i)}
          series={[{ label: 'VO2 max', color: 'linkSoft', data: seriesValues(vo2.series_3m) }]}
          yUnit="decimal"
          height={200}
        />
      </StatsSection>
    </StatsColumns>
  );
}
