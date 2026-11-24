import { useState, type ReactNode } from 'react';
import { Card, CardHeader, PageHeader, SampleDataChip, SegmentedControl, SeriesChart, TopBar, Unavailable } from '../../components';
import type { TrendSeries } from '../../api/generated/TrendSeries';
import type { TrendsResponse } from '../../api/generated/TrendsResponse';
import { fmtFixed, fmtInt, withUnit } from '../../lib/format';
import { useIsDesktop, usePageTitle } from '../../lib/hooks';
import type { AxisUnit } from '../../lib/ticks';
import {
  RANGES,
  breathingSummary,
  durationSummary,
  hrvAside,
  hrvSummary,
  rangeAside,
  rangeLabel,
  rangeXLabels,
  rhrSummary,
  scoreSummary,
  secToMin,
  spo2Summary,
  trendXY,
  type RangeKey,
} from './model';
import { StatsSection } from './desktop';
import { CardSkeleton, ChartSummary, HubLink, LoadError } from './parts';
import { useTrends } from './queries';

interface TrendChartProps {
  trends: TrendsResponse;
  series: TrendSeries;
  name: string;
  summary: string;
  range: RangeKey;
  color: 'link' | 'heart' | 'sleep';
  height: number;
  /** Multiplier from the API unit to the charted unit (seconds to hours). */
  scale?: number;
  band?: [number, number];
  width?: number;
  /** Compact charts (tiles) have no date axis and only two value labels (lowest and highest). */
  compact?: boolean;
  /** What the values are (default whole numbers). */
  unit?: AxisUnit;
}

function TrendChart({ trends, series, name, summary, range, color, height, scale, band, width, compact, unit }: TrendChartProps) {
  const xy = trendXY(series, trends.from, trends.to, scale);
  return (
    <>
      <div className={compact ? 'spark' : undefined}>
        <SeriesChart
          label={`${name} over ${rangeLabel(range)}`}
          summary={summary}
          x={xy?.x ?? []}
          series={[{ label: name, color, data: xy?.y ?? [], ...(width ? { width } : {}) }]}
          height={height}
          {...(band ? { band } : {})}
          {...(unit ? { yUnit: unit } : {})}
          {...(compact ? { sparkline: true } : { xLabels: rangeXLabels(trends) })}
        />
      </div>
    </>
  );
}

function ChartCard({
  title,
  aside,
  summary,
  note,
  series,
  children,
}: {
  title: string;
  aside: ReactNode;
  summary: string;
  note?: string;
  series: TrendSeries;
  children: ReactNode;
}) {
  const hasData = series.points.some((p) => p.value !== null);
  return (
    <Card>
      <CardHeader title={title} aside={aside ?? <Unavailable />} />
      {children}
      {/* SeriesChart already gives screen readers the summary; the visible line is hidden from them. */}
      <ChartSummary hidden={hasData}>
        {note ? `${note} ` : ''}
        {summary}
      </ChartSummary>
    </Card>
  );
}

export function HealthView({ initialRange = '4w' }: { initialRange?: RangeKey }) {
  usePageTitle('Health');
  const desktop = useIsDesktop();
  const [range, setRange] = useState<RangeKey>(initialRange);
  const q = useTrends(range);
  const explore = (
    <div className="hub-links">
      <HubLink to="/sleep" title="Sleep" sub="Last night, stages and vitals" />
      <HubLink to="/sleep?range=4w" title="Score history" sub="Sleep score and time asleep over weeks" />
      <HubLink to="/hrv" title="HRV status" sub="7-day averages and your balanced range" />
      <HubLink to="/training/status" title="Training status" sub="Recovery, productive, strained and more" />
      <HubLink to="/day" title="Day" sub="Heart rate, stress and energy" />
      <HubLink to="/movement" title="Movement" sub="Steps and intensity minutes" />
      <HubLink to="/training" title="Training" sub="Recovery, load and VO2 max" />
      <HubLink to="/body" title="Body" sub="Weight and measurements" />
      <HubLink to="/calendar" title="Calendar" sub="Every day at a glance" />
    </div>
  );
  const body = q.isPending ? (
    <>
      <CardSkeleton lines={4} height={20} />
      <CardSkeleton lines={3} height={20} />
      <CardSkeleton lines={3} height={20} />
    </>
  ) : q.isError && !q.data ? (
    <LoadError what="health trends" error={q.error} onRetry={() => void q.refetch()} />
  ) : q.data ? (
    <Trends trends={q.data} range={range} fetching={q.isFetching} />
  ) : null;
  // A wide screen: the page is a menu of the health pages first (it is not in the sidebar), then the trends.
  if (desktop) {
    return (
      <>
        <PageHeader title="Health" actions={<SampleDataChip sample={q.data?.sample} />} />
        <div className="page" style={{ paddingTop: 0 }} aria-busy={q.isPending}>
          <div className="stats-stack">
            <StatsSection title="Explore">{explore}</StatsSection>
            <StatsSection title="Trends">
              <SegmentedControl label="Time range" options={RANGES} value={range} onChange={setRange} layout="inline" />
              {body}
            </StatsSection>
          </div>
        </div>
      </>
    );
  }
  return (
    <>
      <TopBar variant="large" title="Health" aside={<SampleDataChip sample={q.data?.sample} />} />
      <div className="range-bar">
        <SegmentedControl label="Time range" options={RANGES} value={range} onChange={setRange} layout="grid" />
      </div>
      <div className="page" style={{ paddingTop: 0 }} aria-busy={q.isPending}>
        {body}
        <section aria-labelledby="health-explore" style={{ display: 'flex', flexDirection: 'column', gap: 'var(--gap)' }}>
          <h2 className="section-label" id="health-explore">
            Explore
          </h2>
          {explore}
        </section>
      </div>
    </>
  );
}

function Trends({ trends: t, range, fetching }: { trends: TrendsResponse; range: RangeKey; fetching: boolean }) {
  const desktop = useIsDesktop();
  const band: [number, number] | undefined = t.hrv.baseline_low_ms !== null && t.hrv.baseline_high_ms !== null ? [t.hrv.baseline_low_ms, t.hrv.baseline_high_ms] : undefined;
  const breathing = t.breathing.asleep;
  const spo2 = t.spo2.overnight;
  const durationAvg = secToMin(t.sleep_duration_s.average);
  const durationAside = durationAvg === null ? null : `Avg ${Math.floor(durationAvg / 60)}h ${Math.round(durationAvg % 60)}m`;
  return (
    <div className="grid-panels" style={{ opacity: fetching ? 0.7 : 1 }} aria-label={`Trends over ${rangeLabel(range)}`} role="group">
      <ChartCard title="Overnight HRV" aside={hrvAside(t.hrv)} summary={hrvSummary(t.hrv)} series={t.hrv.series} {...(band ? { note: 'Shaded band is your usual range.' } : {})}>
        <TrendChart
          trends={t}
          series={t.hrv.series}
          name="Overnight HRV"
          summary={hrvSummary(t.hrv)}
          range={range}
          color="link"
          height={desktop ? 150 : 90}
          {...(band ? { band } : {})}
        />
      </ChartCard>

      <ChartCard title="Resting heart rate" aside={rangeAside(t.resting_hr, 'bpm')} summary={rhrSummary(t.resting_hr)} series={t.resting_hr}>
        <TrendChart trends={t} series={t.resting_hr} name="Resting heart rate" summary={rhrSummary(t.resting_hr)} range={range} color="heart" height={desktop ? 150 : 80} />
      </ChartCard>

      <div className="tile-pair">
        <Card padding="tile" className="stat" aria-label="Breathing rate">
          <h2 className="card__title card__title--muted">Breathing rate</h2>
          <span className="stat__value" style={{ fontSize: 22 }}>
            {withUnit(fmtFixed(breathing.average, 1), '/min') ?? <Unavailable />}
          </span>
          <span className="stat__sub">{t.breathing.awake_avg_brpm === null ? 'Asleep' : `Asleep · awake ${fmtFixed(t.breathing.awake_avg_brpm, 1)}`}</span>
          <TrendChart trends={t} series={breathing} name="Breathing rate" summary={breathingSummary(breathing)} range={range} color="link" height={36} width={2} compact unit="decimal" />
        </Card>
        <Card padding="tile" className="stat" aria-label="Blood oxygen">
          <h2 className="card__title card__title--muted">Blood oxygen</h2>
          <span className="stat__value" style={{ fontSize: 22 }}>
            {spo2.average === null ? <Unavailable /> : `${fmtInt(spo2.average)}%`}
          </span>
          <span className="stat__sub">{t.spo2.low_percent === null ? 'Overnight avg' : `Overnight avg · low ${fmtInt(t.spo2.low_percent)}%`}</span>
          <TrendChart trends={t} series={spo2} name="Overnight blood oxygen" summary={spo2Summary(spo2)} range={range} color="link" height={36} width={2} compact />
        </Card>
      </div>

      <ChartCard
        title="Sleep score"
        aside={t.sleep_score.average === null ? null : `Avg ${fmtInt(t.sleep_score.average)}`}
        summary={scoreSummary(t.sleep_score)}
        series={t.sleep_score}
      >
        <TrendChart trends={t} series={t.sleep_score} name="Sleep score" summary={scoreSummary(t.sleep_score)} range={range} color="sleep" height={desktop ? 130 : 70} />
      </ChartCard>

      <ChartCard title="Sleep duration" aside={durationAside} summary={durationSummary(t.sleep_duration_s)} note="Chart in hours." series={t.sleep_duration_s}>
        <TrendChart
          trends={t}
          series={t.sleep_duration_s}
          name="Sleep duration"
          summary={durationSummary(t.sleep_duration_s)}
          range={range}
          color="sleep"
          height={desktop ? 130 : 70}
          scale={1 / 3600}
          unit="hours"
        />
      </ChartCard>
    </div>
  );
}
