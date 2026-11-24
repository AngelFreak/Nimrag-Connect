import { useState, type ReactNode } from 'react';
import { useMe } from '../../account/queries';
import type { ReadinessHistory } from '../../api/generated/ReadinessHistory';
import type { ReadinessFactorKey } from '../../api/generated/ReadinessFactorKey';
import type { ReadinessLevel } from '../../api/generated/ReadinessLevel';
import type { ReadinessRating } from '../../api/generated/ReadinessRating';
import type { ReadinessResponse } from '../../api/generated/ReadinessResponse';
import { BarSeries, Card, CardHeader, Legend, PageHeader, Ring, SampleDataChip, SegmentedControl, TopBar } from '../../components';
import { fmtShortDay, todayIn } from '../../lib/dates';
import { useIsDesktop, usePageTitle } from '../../lib/hooks';
import { FactorList, FactorRow, Headline, StatsColumns, StatsSection } from './desktop';
import { CardSkeleton, LoadError } from './parts';
import { useReadiness, useReadinessHistory } from './readinessQueries';
import {
  ESTIMATE_LABEL,
  FACTOR_META,
  HISTORY_NAME,
  HISTORY_OPTIONS,
  LEVELS,
  LEVEL_COLOR,
  LEVEL_NAME,
  LEVEL_RANGE,
  RATING_COLOR,
  RATING_NAME,
  factorSub,
  factorWord,
  historyChart,
  historyWindow,
  type HistoryKey,
} from './readiness';
import './readiness.css';

// The Readiness page (/training/readiness): Nimrag's own estimate of how ready you are today, from
// six factors it explains one by one, and the estimate day by day. It is not Garmin's Training
// Readiness (the watch writes none) and the page says so under the score.

function Dot({ color }: { color: string }) {
  return <span className="rate-dot" aria-hidden="true" style={{ background: color }} />;
}

/** A rating as a coloured dot and its word: the word carries the meaning. A factor says it in its own words. */
export function RatingWord({ rating, factor }: { rating: ReadinessRating | null; factor?: ReadinessFactorKey }) {
  if (rating === null) return <span className="rate rate--none">Not available</span>;
  return (
    <span className="rate">
      <Dot color={RATING_COLOR[rating]} />
      {factor ? factorWord(factor, rating) : RATING_NAME[rating]}
    </span>
  );
}

export function ReadinessView() {
  usePageTitle('Readiness');
  const desktop = useIsDesktop();
  const q = useReadiness();
  const sample = q.data?.sample;
  return (
    <>
      {desktop ? (
        <PageHeader title="Readiness" actions={<SampleDataChip sample={sample} />}>
          <p className="stats-note">{ESTIMATE_LABEL}</p>
        </PageHeader>
      ) : (
        <TopBar variant="back" backTo="/training" backLabel="Back to Training" title="Readiness" subtitle="Nimrag estimate" aside={<SampleDataChip sample={sample} />} />
      )}
      <div className="page" style={{ paddingTop: 0 }} aria-busy={q.isPending}>
        {q.isPending ? (
          <>
            <CardSkeleton lines={3} height={28} />
            <CardSkeleton lines={4} height={20} />
          </>
        ) : q.isError && !q.data ? (
          <LoadError what="the readiness estimate" error={q.error} onRetry={() => void q.refetch()} />
        ) : q.data ? (
          desktop ? <BodyDesktop data={q.data} /> : <BodyPhone data={q.data} />
        ) : null}
      </div>
    </>
  );
}

// ---- the score ------------------------------------------------------------------------------------

function Hero({ data }: { data: ReadinessResponse }) {
  const { score, level } = data;
  const color = level ? LEVEL_COLOR[level] : 'var(--accent)';
  return (
    <div className="ready-hero">
      <Ring value={score} max={100} label="Readiness estimate" color={color} size={156} strokeWidth={12}>
        {score === null ? '' : String(score)}
      </Ring>
      {level ? (
        <p className="ready-hero__level">
          <Dot color={color} />
          <span>
            <span className="sr-only">Level: </span>
            {LEVEL_NAME[level]}
          </span>
        </p>
      ) : null}
      <p className="ready-hero__tag">{ESTIMATE_LABEL}</p>
      {score !== null && data.headline && data.sentence ? (
        <Headline title={data.headline}>{data.sentence}</Headline>
      ) : (
        <Headline title="No estimate yet">{data.unavailable ?? 'Readiness needs a few days of data.'}</Headline>
      )}
    </div>
  );
}

function Factors({ data }: { data: ReadinessResponse }) {
  return (
    <FactorList tight label="What the estimate is made from">
      {data.factors.map((f) => (
        <FactorRow
          key={f.key}
          to={FACTOR_META[f.key].to}
          name={FACTOR_META[f.key].name}
          sub={factorSub(f)}
          rating={<RatingWord rating={f.rating} factor={f.key} />}
        />
      ))}
    </FactorList>
  );
}

function FactorsNote({ data }: { data: ReadinessResponse }) {
  return (
    <p className="stats-note">
      {data.present} of 6 factors available. A missing factor is left out and the others count for more. The score needs at least 4 factors and 3 days of data.
    </p>
  );
}

// ---- history ----------------------------------------------------------------------------------------

function useHistory(key: HistoryKey) {
  const me = useMe();
  const today = todayIn(me.data?.timezone);
  const { from, to } = historyWindow(key, today);
  return useReadinessHistory(from, to);
}

function HistoryChart({ history, height }: { history: ReadinessHistory; height: number }) {
  const chart = historyChart(history.days, history.average);
  if (chart.scored === 0) return <p className="stat__sub">No estimate yet in this period.</p>;
  return (
    <BarSeries
      values={chart.values}
      colors={chart.colors}
      labels={chart.labels}
      {...(chart.axis.length > 0 ? { axisLabels: chart.axis } : {})}
      label="Readiness estimate by day"
      summary={chart.summary}
      summaryVisible
      max={100}
      height={height}
    />
  );
}

function LevelKey() {
  return (
    <Legend
      items={LEVELS.map((l: ReadinessLevel) => ({ key: l, label: `${LEVEL_NAME[l]} ${LEVEL_RANGE[l]}`, color: LEVEL_COLOR[l] }))}
    />
  );
}

function HistoryBody({ height }: { height: number }) {
  const [range, setRange] = useState<HistoryKey>('4w');
  const h = useHistory(range);
  return (
    <>
      <SegmentedControl label="Period" options={HISTORY_OPTIONS} value={range} onChange={setRange} layout="inline" />
      <div style={{ opacity: h.isPlaceholderData ? 0.7 : 1 }} aria-busy={h.isPending}>
        {h.isPending ? (
          <CardSkeleton lines={2} height={20} />
        ) : h.isError && !h.data ? (
          <LoadError what="the readiness history" error={h.error} onRetry={() => void h.refetch()} />
        ) : h.data ? (
          <>
            <HistoryChart history={h.data} height={height} />
            <LevelKey />
          </>
        ) : null}
      </div>
      <p className="sr-only">Showing {HISTORY_NAME[range]}.</p>
    </>
  );
}

// ---- layouts ----------------------------------------------------------------------------------------

function BodyPhone({ data }: { data: ReadinessResponse }) {
  return (
    <div className="page-stack">
      <Card padding="xl">
        <Hero data={data} />
      </Card>
      <Section title="What it is made from">
        <Card padding="lg">
          <Factors data={data} />
          <FactorsNote data={data} />
        </Card>
      </Section>
      <Card padding="lg">
        <CardHeader title="Day by day" tone="muted" />
        <HistoryBody height={110} />
      </Card>
      <About date={data.date} />
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="page-stack" aria-label={title}>
      <h2 className="section-label">{title}</h2>
      {children}
    </section>
  );
}

function BodyDesktop({ data }: { data: ReadinessResponse }) {
  return (
    <StatsColumns
      side={
        <StatsSection title="Today" aside={fmtShortDay(data.date)}>
          <Hero data={data} />
        </StatsSection>
      }
    >
      <StatsSection title="What it is made from">
        <Factors data={data} />
        <FactorsNote data={data} />
      </StatsSection>
      <StatsSection title="Day by day">
        <HistoryBody height={200} />
      </StatsSection>
      <About date={data.date} />
    </StatsColumns>
  );
}

function About({ date }: { date: string }) {
  return (
    <p className="stats-note">
      Calculated by Nimrag from your sleep, recovery time, HRV, training load and stress on {fmtShortDay(date)}. Garmin’s own score is not available on the Instinct 2.
    </p>
  );
}
