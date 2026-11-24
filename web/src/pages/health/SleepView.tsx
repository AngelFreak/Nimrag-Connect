import { useState, type CSSProperties } from 'react';
import { Navigate, useNavigate } from 'react-router-dom';
import {
  BarSeries,
  Card,
  CardHeader,
  DataList,
  EmptyState,
  Legend,
  PageHeader,
  SampleDataChip,
  SegmentedControl,
  Skeleton,
  StageTimeline,
  StatStrip,
  StatTile,
  TopBar,
} from '../../components';
import type { SleepNight } from '../../api/generated/SleepNight';
import type { SleepNightsResponse } from '../../api/generated/SleepNightsResponse';
import { addDays, browserToday, fmtShortDay } from '../../lib/dates';
import { fmtFixed, fmtInt, withUnit } from '../../lib/format';
import { usePageTitle, useIsDesktop } from '../../lib/hooks';
import { axisDates, fmtSeconds, nightBars, nightsSummary, ratingWord, scoreLabel, sleepTicks, stageSpans, stageSummary } from './model';
import { FactorList, FactorRow, Headline, ScoreHero, StatsColumns, StatsSection } from './desktop';
import { CardSkeleton, ChartSummary, LoadError } from './parts';
import { SleepRangeBar, SleepToolbar } from './SleepRangeView';
import { useSleepNight, useSleepNights, type NightCount } from './queries';
import { clock } from './time';

const COUNTS = [
  { value: '7', label: '7 nights' },
  { value: '14', label: '14 nights' },
  { value: '30', label: '30 nights' },
] as const;

/** `/sleep` without a date: go to the latest night the server has. */
export function LatestNight() {
  usePageTitle('Sleep');
  const list = useSleepNights(7);
  const latest = list.data?.nights.at(-1)?.date;
  const desktop = useIsDesktop();
  const navigate = useNavigate();
  if (latest) return <Navigate to={`/sleep/${latest}`} replace />;
  const today = browserToday();
  return (
    <>
      {desktop ? (
        <PageHeader title="Sleep" actions={<SampleDataChip sample={list.data?.sample} />}>
          <SleepToolbar from={today} to={today} atToday onStep={() => {}} unit="night" value="1d" onChange={(k) => void navigate(k === '1d' ? '/sleep' : `/sleep?range=${k}`)} />
        </PageHeader>
      ) : (
        <>
          <TopBar variant="back" backTo="/health" title="Sleep" aside={<SampleDataChip sample={list.data?.sample} />} />
          <SleepRangeBar value="1d" />
        </>
      )}
      <div className="page" style={desktop ? { paddingTop: 0 } : undefined} aria-busy={list.isPending}>
        {list.isPending ? <CardSkeleton lines={3} height={28} /> : null}
        {list.isError ? <LoadError what="sleep" error={list.error} onRetry={() => void list.refetch()} /> : null}
        {list.data ? <EmptyState title="No sleep recorded yet">Nights appear here after the watch has synced.</EmptyState> : null}
      </div>
    </>
  );
}

export function SleepView({ date }: { date: string }) {
  const title = `Sleep · ${fmtShortDay(date)}`;
  usePageTitle(title);
  const [count, setCount] = useState<NightCount>(7);
  const night = useSleepNight(date);
  const list = useSleepNights(count);

  const latest = list.data?.nights.at(-1)?.date ?? null;
  // A night belongs to the day it ends on, so the newest possible one is today's.
  const hasNext = latest !== null ? date < latest : date < browserToday();
  const sample = night.data?.sample ?? list.data?.sample;
  const desktop = useIsDesktop();
  const navigate = useNavigate();

  if (desktop) {
    return (
      <>
        <PageHeader title="Sleep" actions={<SampleDataChip sample={sample} />}>
          <SleepToolbar
            from={date}
            to={date}
            atToday={!hasNext}
            onStep={(d) => void navigate(`/sleep/${addDays(date, d)}`)}
            unit="night"
            value="1d"
            onChange={(k) => void navigate(k === '1d' ? '/sleep' : `/sleep?range=${k}`)}
          />
        </PageHeader>
        <div className="page" style={{ paddingTop: 0 }} aria-busy={night.isPending}>
          {night.isPending ? (
            <div className="stats-cols">
              <CardSkeleton lines={2} height={28} />
              <CardSkeleton lines={4} height={20} />
            </div>
          ) : night.isError ? (
            <LoadError what="this night" error={night.error} onRetry={() => void night.refetch()} />
          ) : (
            <div className="stats-stack">
              {night.data.night === null ? (
                <EmptyState title="No sleep recorded for this night">The watch did not record sleep that night. Try the previous or next night.</EmptyState>
              ) : (
                <NightDesktop night={night.data.night} />
              )}
              <LastNightsDesktop count={count} onCount={setCount} data={list.data} pending={list.isPending} error={list.isError ? list.error : null} onRetry={() => void list.refetch()} />
            </div>
          )}
        </div>
      </>
    );
  }

  return (
    <>
      <TopBar
        variant="back"
        backTo="/health"
        backLabel="Back to Health"
        title="Sleep"
        subtitle={`Night of ${fmtShortDay(addDays(date, -1))}`}
        aside={desktop ? <SampleDataChip sample={sample} /> : undefined}
        prev={{ to: `/sleep/${addDays(date, -1)}` }}
        next={hasNext ? { to: `/sleep/${addDays(date, 1)}` } : null}
        prevLabel="Previous night"
        nextLabel="Next night"
      />
      <SleepRangeBar value="1d" />
      <div className="page" aria-busy={night.isPending}>
        {!desktop && sample ? (
          <div>
            <SampleDataChip sample={sample} />
          </div>
        ) : null}
        <div className="grid-panels" style={{ alignItems: 'start' }}>
          {night.isPending ? (
            <>
              <CardSkeleton lines={2} height={28} />
              <CardSkeleton lines={4} height={20} />
            </>
          ) : night.isError ? (
            <div style={{ gridColumn: '1 / -1' }}>
              <LoadError what="this night" error={night.error} onRetry={() => void night.refetch()} />
            </div>
          ) : night.data.night === null ? (
            <div style={{ gridColumn: '1 / -1' }}>
              <EmptyState title="No sleep recorded for this night">The watch did not record sleep that night. Try the previous or next night.</EmptyState>
            </div>
          ) : (
            <NightCards night={night.data.night} />
          )}
          <LastNights count={count} onCount={setCount} data={list.data} pending={list.isPending} error={list.isError ? list.error : null} onRetry={() => void list.refetch()} />
        </div>
      </div>
    </>
  );
}

function NightCards({ night }: { night: SleepNight }) {
  const desktop = useIsDesktop();
  const { spans, totalMinutes } = stageSpans(night);
  const label = scoreLabel(night.score_label);
  const spo2 = night.spo2_avg_percent;
  const vitalCols = desktop ? (spo2 === null ? 3 : 4) : spo2 === null ? 3 : 2;
  // Desktop: score left, the stage chart on the right over two rows, vitals across the full width.
  const at = (col: string, row: string): CSSProperties | undefined => (desktop ? { gridColumn: col, gridRow: row } : undefined);
  return (
    <>
      <div style={at('1', '1')}>
        <Card padding="lg" row>
          <div className="stat stat--lg">
            <span className="stat__label">Time asleep</span>
            <span className="stat__value">{fmtSeconds(night.duration_s) ?? <span className="unavailable">unavailable</span>}</span>
            <span className="stat__sub">
              {clock(night.start)} to {clock(night.end)}
            </span>
          </div>
          <div className="stat stat--lg" style={{ alignItems: 'flex-end' }}>
            <span className="stat__label">Score</span>
            <span className="stat__value tone-sleep">{night.score === null ? <span className="unavailable">unavailable</span> : fmtInt(night.score)}</span>
            {label ? <span className="stat__sub tone-sleep">{label}</span> : null}
          </div>
        </Card>
      </div>

      <div style={at('2', '1 / span 2')}>
        <Card padding="lg">
          <CardHeader title="Stages through the night" tone="muted" />
          {night.stages_available && spans.length > 0 ? (
            <StageTimeline spans={spans} totalMinutes={totalMinutes} label="Sleep stages through the night" ticks={sleepTicks(night)} />
          ) : (
            <p className="stat__sub">Stages were not recorded for this night.</p>
          )}
          <ChartSummary>{stageSummary(night)}</ChartSummary>
          <Legend
            grid
            items={[
              { key: 'deep', label: 'Deep', color: 'var(--stage-deep)', value: fmtSeconds(night.deep_s), hasValue: true },
              { key: 'light', label: 'Light', color: 'var(--stage-light)', value: fmtSeconds(night.light_s), hasValue: true },
              { key: 'rem', label: 'REM', color: 'var(--stage-rem)', value: fmtSeconds(night.rem_s), hasValue: true },
              { key: 'awake', label: 'Awake', color: 'var(--stage-awake)', value: fmtSeconds(night.awake_s), hasValue: true },
            ]}
          />
        </Card>
      </div>

      <div style={{ display: 'grid', gap: 'var(--gap)', gridTemplateColumns: `repeat(${vitalCols}, minmax(0, 1fr))`, ...(desktop ? { gridColumn: '1 / -1', gridRow: '3' } : {}) }}>
        <StatTile size="sm" label="HRV" value={withUnit(fmtInt(night.hrv_ms), 'ms')} />
        <StatTile size="sm" label="Lowest HR" value={withUnit(fmtInt(night.lowest_hr_bpm), 'bpm')} />
        <StatTile size="sm" label="Breathing" value={withUnit(fmtFixed(night.breathing_brpm, 1), '/min')} />
        {spo2 !== null ? (
          <StatTile size="sm" label="Blood oxygen" value={`${fmtInt(spo2)}%`} sub={night.spo2_low_percent === null ? undefined : `Low ${fmtInt(night.spo2_low_percent)}%`} />
        ) : null}
      </div>

      <div style={at('1', '2')}>
        <Card padding="lg" className="night-details">
          <CardHeader title="Night details" tone="muted" />
          <DataList
            items={[
              { label: 'Time in bed', value: fmtSeconds(night.time_in_bed_s) },
              { label: 'Awakenings', value: fmtInt(night.awakenings) },
              { label: 'Average stress', value: fmtInt(night.avg_stress) },
              { label: 'Quality score', value: fmtInt(night.scores.quality) },
              { label: 'Duration score', value: fmtInt(night.scores.duration) },
              { label: 'Recovery score', value: fmtInt(night.scores.recovery) },
            ]}
          />
        </Card>
      </div>
    </>
  );
}

function LastNights({
  count,
  onCount,
  data,
  pending,
  error,
  onRetry,
}: {
  count: NightCount;
  onCount: (c: NightCount) => void;
  data: SleepNightsResponse | undefined;
  pending: boolean;
  error: unknown;
  onRetry: () => void;
}) {
  const bars = data ? nightBars(data.nights, count) : null;
  const avg = data ? fmtSeconds(data.average_duration_s) : null;
  const summary = data ? nightsSummary(data.nights, count, data.average_score) : null;
  return (
    <div style={{ gridColumn: '1 / -1' }}>
      <Card padding="lg" className="last-nights">
        <CardHeader title={`Last ${count} nights`} tone="muted" aside={avg ? `Average ${avg}` : undefined} />
        <SegmentedControl label="Number of nights" options={COUNTS} value={String(count) as '7' | '14' | '30'} onChange={(v) => onCount(Number(v) as NightCount)} />
        {pending ? <Skeleton height={84} /> : null}
        {error && !data ? <LoadError what="the last nights" error={error} onRetry={onRetry} /> : null}
        {bars && bars.dates.length > 0 ? (
          <>
            <BarSeries
              values={bars.values}
              labels={bars.labels}
              axisLabels={count === 7 ? undefined : axisDates(bars.dates)}
              label={`Sleep duration for the last ${count} nights`}
              yUnit="minutes"
              color="var(--stage-deep)"
              highlightIndex={bars.values.length - 1}
              summary={summary ?? undefined}
              summaryVisible
              height={count === 7 ? 84 : 96}
            />
          </>
        ) : null}
        {data && bars && bars.dates.length === 0 ? <p className="stat__sub">No nights recorded yet.</p> : null}
      </Card>
    </div>
  );
}

// ---- wide screen ------------------------------------------------------------------------------------

/** Rows of "Sleep Score Factors": what was measured and how the sub-score rates. Rows without any data are left out. */
function factorRows(night: SleepNight) {
  const sc = night.scores;
  const rows: { key: string; name: string; measure: string | null; score: number | null }[] = [
    { key: 'duration', name: 'Duration', measure: fmtSeconds(night.duration_s), score: sc.duration },
    { key: 'quality', name: 'Quality', measure: null, score: sc.quality },
    { key: 'recovery', name: 'Recovery', measure: night.avg_stress === null ? null : `${fmtInt(night.avg_stress)} avg stress`, score: sc.recovery },
    { key: 'deep', name: 'Deep', measure: fmtSeconds(night.deep_s), score: sc.deep },
    { key: 'light', name: 'Light', measure: fmtSeconds(night.light_s), score: sc.light },
    { key: 'rem', name: 'REM', measure: fmtSeconds(night.rem_s), score: sc.rem },
  ];
  return rows.filter((r) => r.measure !== null || r.score !== null);
}

function NightDesktop({ night }: { night: SleepNight }) {
  const { spans, totalMinutes } = stageSpans(night);
  const label = scoreLabel(night.score_label);
  const spo2 = night.spo2_avg_percent;
  const rows = factorRows(night);
  const awake = fmtSeconds(night.awake_s);
  const awakenings = night.awakenings === null ? null : `${night.awakenings} awakening${night.awakenings === 1 ? '' : 's'}`;
  const na = <span className="unavailable">unavailable</span>;
  return (
    <StatsColumns
      side={
        <>
          <StatsSection title="Sleep Score">
            <ScoreHero value={night.score === null ? na : fmtInt(night.score)} of={night.score === null ? undefined : '100'} label="Score" tone="tone-sleep" />
            <StatStrip
              items={[
                { key: 'rating', label: 'Rating', value: label },
                { key: 'asleep', label: 'Time asleep', value: fmtSeconds(night.duration_s) },
              ]}
            />
          </StatsSection>
          <Headline title={`${clock(night.start)} to ${clock(night.end)}`}>{stageSummary(night)}</Headline>
          <StatStrip
            items={[
              { key: 'bed', label: 'Time in bed', value: fmtSeconds(night.time_in_bed_s) },
              { key: 'awakenings', label: 'Awakenings', value: fmtInt(night.awakenings) },
              { key: 'stress', label: 'Average stress', value: fmtInt(night.avg_stress) },
            ]}
          />
        </>
      }
    >
      <StatsSection title="Sleep Score Factors">
        <FactorList label="Sleep score factors">
          {rows.map((r) => (
            <FactorRow
              key={r.key}
              name={r.name}
              sub={[r.measure, r.score === null ? null : `score ${fmtInt(r.score)}`].filter(Boolean).join(' · ') || undefined}
              rating={ratingWord(r.score) ?? undefined}
            />
          ))}
          {awake !== null || awakenings !== null ? <FactorRow key="awake" name="Awake" sub={[awake, awakenings].filter(Boolean).join(' · ')} /> : null}
        </FactorList>
      </StatsSection>

      <StatsSection title="Stages through the night">
        {night.stages_available && spans.length > 0 ? (
          <StageTimeline spans={spans} totalMinutes={totalMinutes} label="Sleep stages through the night" ticks={sleepTicks(night)} />
        ) : (
          <p className="stat__sub">Stages were not recorded for this night.</p>
        )}
        <Legend
          grid
          items={[
            { key: 'deep', label: 'Deep', color: 'var(--stage-deep)', value: fmtSeconds(night.deep_s), hasValue: true },
            { key: 'light', label: 'Light', color: 'var(--stage-light)', value: fmtSeconds(night.light_s), hasValue: true },
            { key: 'rem', label: 'REM', color: 'var(--stage-rem)', value: fmtSeconds(night.rem_s), hasValue: true },
            { key: 'awake', label: 'Awake', color: 'var(--stage-awake)', value: fmtSeconds(night.awake_s), hasValue: true },
          ]}
        />
      </StatsSection>

      <StatsSection title="Overnight vitals">
        <StatStrip
          items={[
            { key: 'hrv', label: 'HRV', value: withUnit(fmtInt(night.hrv_ms), 'ms') },
            { key: 'lowest', label: 'Lowest HR', value: withUnit(fmtInt(night.lowest_hr_bpm), 'bpm') },
            { key: 'breathing', label: 'Breathing', value: withUnit(fmtFixed(night.breathing_brpm, 1), '/min') },
            ...(spo2 === null ? [] : [{ key: 'spo2', label: night.spo2_low_percent === null ? 'Blood oxygen' : `Blood oxygen, low ${fmtInt(night.spo2_low_percent)}%`, value: `${fmtInt(spo2)}%` }]),
          ]}
        />
      </StatsSection>
    </StatsColumns>
  );
}

function LastNightsDesktop({
  count,
  onCount,
  data,
  pending,
  error,
  onRetry,
}: {
  count: NightCount;
  onCount: (c: NightCount) => void;
  data: SleepNightsResponse | undefined;
  pending: boolean;
  error: unknown;
  onRetry: () => void;
}) {
  const bars = data ? nightBars(data.nights, count) : null;
  const avg = data ? fmtSeconds(data.average_duration_s) : null;
  const summary = data ? nightsSummary(data.nights, count, data.average_score) : null;
  return (
    <StatsSection title={`Last ${count} nights`} aside={avg ? `Average ${avg}` : undefined}>
      <SegmentedControl label="Number of nights" options={COUNTS} value={String(count) as '7' | '14' | '30'} onChange={(v) => onCount(Number(v) as NightCount)} />
      {pending ? <Skeleton height={84} /> : null}
      {error && !data ? <LoadError what="the last nights" error={error} onRetry={onRetry} /> : null}
      {bars && bars.dates.length > 0 ? (
        <BarSeries
          values={bars.values}
          labels={bars.labels}
          axisLabels={count === 7 ? undefined : axisDates(bars.dates)}
          label={`Sleep duration for the last ${count} nights`}
          yUnit="minutes"
          color="var(--stage-deep)"
          highlightIndex={bars.values.length - 1}
          summary={summary ?? undefined}
          summaryVisible
          height={200}
        />
      ) : null}
      {data && bars && bars.dates.length === 0 ? <p className="stat__sub">No nights recorded yet.</p> : null}
    </StatsSection>
  );
}
