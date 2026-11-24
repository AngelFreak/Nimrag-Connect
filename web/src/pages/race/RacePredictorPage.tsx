import { useState } from 'react';
import type { RacePredictionsResponse } from '../../api/generated/RacePredictionsResponse';
import { Card, CardHeader, FilterChips, PageHeader, SampleDataChip, SegmentedControl, SeriesChart, TopBar } from '../../components';
import { useIsDesktop, usePageTitle } from '../../lib/hooks';
import { PageError, PageSkeleton } from '../body/common';
import {
  DISTANCES,
  DISTANCE_CHIP,
  ESTIMATE_LABEL,
  HISTORY_OPTIONS,
  basisText,
  distanceName,
  fmtChange,
  fmtPace,
  fmtRaceTime,
  pointsFor,
  raceChart,
  type Distance,
  type HistoryKey,
} from './model';
import { useRacePredictions } from './queries';
import './race.css';

// The race predictor (/training/race-predictor): Nimrag's own estimate of 5 km, 10 km, half marathon
// and marathon times, from your best efforts of the last 6 weeks and your VO2 max, and how the
// estimate moved week by week. It is not Garmin's Race Predictor and the page says so.

export function RacePredictorPage() {
  usePageTitle('Race predictor');
  const desktop = useIsDesktop();
  const q = useRacePredictions();
  const data = q.data;
  return (
    <>
      {desktop ? (
        <PageHeader title="Race predictor" actions={<SampleDataChip sample={data?.sample} />} />
      ) : (
        <TopBar variant="back" backTo="/training" backLabel="Back to Training" title="Race predictor" subtitle="Nimrag estimate" aside={<SampleDataChip sample={data?.sample} />} />
      )}
      {q.isError && !data ? (
        <PageError title="The race predictions could not be loaded" error={q.error} onRetry={() => void q.refetch()} />
      ) : !data ? (
        <PageSkeleton cards={3} />
      ) : (
        <RaceBody data={data} />
      )}
    </>
  );
}

export function RaceBody({ data }: { data: RacePredictionsResponse }) {
  const desktop = useIsDesktop();
  return (
    <div className="page" style={desktop ? { paddingTop: 0 } : undefined}>
      {data.predictions.length === 0 ? (
        <EmptyState />
      ) : (
        <>
          <Predictions data={data} />
          <HistoryCard data={data} />
        </>
      )}
      <Note data={data} />
    </div>
  );
}

function EmptyState() {
  return (
    <Card padding="xl">
      <p className="headline__title" style={{ margin: 0 }}>
        No estimate yet
      </p>
      <p className="stats-note" style={{ marginTop: 8 }}>
        A run with GPS in the last 6 weeks is needed.
      </p>
    </Card>
  );
}

function Predictions({ data }: { data: RacePredictionsResponse }) {
  return (
    <ul className="race-rows" aria-label="Predicted race times">
      {data.predictions.map((p) => {
        const change = fmtChange(p.change_4w_s);
        return (
          <Card as="li" key={p.distance_m} padding="lg" className="race-row">
            <h2 className="race-row__name">{distanceName(p.distance_m)}</h2>
            <p className="race-row__time">{fmtRaceTime(p.seconds)}</p>
            <p className="race-row__meta">
              <span>{fmtPace(p.pace_s_per_km)}</span>
            </p>
            <p className={change ? `race-row__meta race-row__change race-row__change--${change.tone}` : 'race-row__meta race-row__change'}>{change ? change.text : 'Nothing to compare with 4 weeks ago'}</p>
          </Card>
        );
      })}
    </ul>
  );
}

function HistoryCard({ data }: { data: RacePredictionsResponse }) {
  const [range, setRange] = useState<HistoryKey>('6m');
  const [distance, setDistance] = useState<Distance>(5000);
  const chart = raceChart(pointsFor(data.history, range), distance);
  const desktop = useIsDesktop();
  return (
    <Card padding="lg">
      <CardHeader title="History" tone="muted" />
      <div className="race-chart__controls">
        <FilterChips label="Distance" options={DISTANCES.map((d) => ({ value: String(d), label: DISTANCE_CHIP[d] }))} value={String(distance)} onChange={(v) => setDistance(Number(v) as Distance)} />
        <SegmentedControl label="Period" options={HISTORY_OPTIONS} value={range} onChange={setRange} layout="inline" />
      </div>
      <div className="race-chart__plot">
        {chart.count === 0 ? (
          <p className="stat__sub">{chart.summary}</p>
        ) : (
          <SeriesChart
            label={`${distanceName(distance)} estimate, ${range === '4w' ? 'last 4 weeks' : 'last 6 months'}`}
            summary={chart.summary}
            summaryVisible
            x={chart.x}
            series={[{ label: distanceName(distance), color: 'energy', data: chart.y, points: true }]}
            height={desktop ? 200 : 140}
            yUnit="pace"
            yReverse
            xLabels={chart.xLabels}
          />
        )}
      </div>
      <p className="stats-note">Faster is up. Weekly estimates, as of each Sunday.</p>
    </Card>
  );
}

function Note({ data }: { data: RacePredictionsResponse }) {
  const based = basisText(data.basis, data.vo2max);
  return (
    <p className="race-note stats-note">
      <span className="race-note__label">{ESTIMATE_LABEL}</span>
      {based ? <span>{based}</span> : null}
      <span>Worked out by Nimrag with Riegel’s formula and the Daniels and Gilbert VO2 max tables. Treat it as a guide, not a promise.</span>
    </p>
  );
}
