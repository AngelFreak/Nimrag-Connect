import { Link } from 'react-router-dom';
import type { MetricRange } from '../../api/generated/MetricRange';
import { BarSeries, Card, CardHeader, EmptyState, Legend, SeriesChart, StackedBars, StatStrip, StatTile, ValueOrUnavailable } from '../../components';
import { useIsDesktop } from '../../lib/hooks';
import { chartHasData, rangeChart, rangeRows, rangeSummary, statText, tileStats, type ChartSpec, type RangeRow } from './model';
import { metricHref, type MetricDef } from './period';

// A multi-day tab: the chart of the period, its figures, and a row for every day or week with a
// value, newest first. A row opens that day (on 1d) or that week (on 7d).

export function RangeBody({ def, data, today }: { def: MetricDef; data: MetricRange; today: string }) {
  const spec = rangeChart(def.key, data, today);
  const rows = rangeRows(def, data, today);
  const tiles = tileStats(def.key, data);
  const week = data.bucket === 'week';
  const desktop = useIsDesktop();
  const chart = chartHasData(spec) ? (
    <Plot spec={spec} label={`${def.title} ${week ? 'by week' : 'by day'}`} summary={rangeSummary(def, data)} />
  ) : (
    <EmptyState title={`No ${def.title.toLowerCase()} in this period`}>The watch has not recorded any for these days. Try an earlier period.</EmptyState>
  );
  const rowList =
    rows.length > 0 ? (
      <section aria-labelledby="metric-rows" className="page-stack">
        <h2 className="section-label" id="metric-rows">
          {week ? 'Weeks, newest first' : 'Days, newest first'}
        </h2>
        <div className="metric-rows">
          {rows.map((r) => (
            <Row key={r.start} def={def} row={r} today={today} />
          ))}
        </div>
      </section>
    ) : null;
  // A wide screen: a section title, the chart without a card, and the figures in a strip under it.
  if (desktop) {
    const items = tiles.map((s) => {
      const t = statText(s);
      return { key: s.key, label: s.label, value: t.value && t.unit ? `${t.value} ${t.unit}` : t.value };
    });
    return (
      <>
        <section aria-labelledby="metric-chart" className="page-stack">
          <h2 className="section-title" id="metric-chart">
            {week ? 'Week by week' : 'Day by day'}
          </h2>
          {chart}
          {items.length > 0 ? <StatStrip items={items} /> : null}
        </section>
        {rowList}
      </>
    );
  }
  return (
    <>
      <div className="metric-top">
        <Card padding="lg">
          <CardHeader title={week ? 'Week by week' : 'Day by day'} tone="muted" />
          {chart}
        </Card>
        <div className="metric-tiles">
          {tiles.map((s) => {
            const t = statText(s);
            return <StatTile key={s.key} size="sm" label={s.label} value={t.value} {...(t.unit ? { sub: t.unit } : {})} />;
          })}
        </div>
      </div>
      {rowList}
    </>
  );
}

function Plot({ spec, label, summary }: { spec: ChartSpec; label: string; summary: string }) {
  const desktop = useIsDesktop();
  const height = desktop ? 240 : 150;
  if (spec.kind === 'stacked') {
    return (
      <>
        <StackedBars parts={spec.parts} labels={spec.labels} {...(spec.axisLabels ? { axisLabels: spec.axisLabels } : {})} label={`${label}, minutes in each stress level`} summary={summary} height={height} />
        <Legend items={spec.legend} />
        <p className="chart-note">Average stress</p>
        <SeriesChart
          label={`${label}, average stress`}
          summary={`${summary} Average stress for each ${label.endsWith('by week') ? 'week' : 'day'} on a scale of 0 to 100.`}
          x={spec.average.x}
          series={spec.average.series}
          xRange={spec.average.xRange}
          xLabels={spec.average.xLabels}
          yUnit={spec.average.yUnit}
          {...(spec.average.yRange ? { yRange: spec.average.yRange } : {})}
          height={desktop ? 140 : 100}
        />
      </>
    );
  }
  return (
    <>
      {spec.kind === 'line' ? (
        <SeriesChart
          label={label}
          summary={summary}
          x={spec.x}
          series={spec.series}
          xRange={spec.xRange}
          xLabels={spec.xLabels}
          yUnit={spec.yUnit}
          {...(spec.yRange ? { yRange: spec.yRange } : {})}
          {...(spec.band ? { band: spec.band } : {})}
          height={height}
        />
      ) : (
        <BarSeries
          values={spec.values}
          labels={spec.labels}
          label={label}
          summary={summary}
          color={spec.color}
          yUnit={spec.yUnit}
          {...(spec.axisLabels ? { axisLabels: spec.axisLabels } : {})}
          {...(spec.goal !== undefined ? { goal: spec.goal } : {})}
          {...(spec.belowGoalColor ? { belowGoalColor: spec.belowGoalColor } : {})}
          height={height}
        />
      )}
      <Legend items={spec.legend} />
    </>
  );
}

function Row({ def, row, today }: { def: MetricDef; row: RangeRow; today: string }) {
  const inner = (
    <>
      <span className="list-row__main">
        <span className="list-row__title">{row.title}</span>
        {row.sub ? <span className="list-row__sub">{row.sub}</span> : null}
      </span>
      <span className="list-row__end">
        <span className="list-row__value">
          <ValueOrUnavailable value={row.value} />
        </span>
        {row.valueSub ? <span className="list-row__sub">{row.valueSub}</span> : null}
      </span>
    </>
  );
  return row.target ? (
    <Link to={metricHref(def.key, row.target.range, row.target.end, today)} className="list-row">
      {inner}
    </Link>
  ) : (
    <div className="list-row">{inner}</div>
  );
}
