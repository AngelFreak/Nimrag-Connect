import { useCallback } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useMe } from '../../account/queries';
import { SampleDataChip, TopBar } from '../../components';
import { todayIn } from '../../lib/dates';
import { usePageTitle } from '../../lib/hooks';
import { CardSkeleton, LoadError } from '../health/parts';
import { PeriodNav, RangeBar, type PeriodOption } from '../health/period';
import { DayBody } from './DayBody';
import { RangeBody } from './RangeBody';
import { METRICS, RANGE_DAYS, metricWindow, stepAnchor, type MetricDef, type MetricKey, type MetricWindow, type RangeKey, type ServerRange } from './period';
import { useMetricRange } from './queries';
import './metrics.css';

// A metric's detail page, opened from a tile on Today: /metrics/steps?range=7d&end=2026-11-14. The tab
// and the day the period ends on are in the address, so a page can be linked and the back button
// walks back through the periods. Period tabs and their windows are described in `period.ts`.

interface MetricPeriod extends MetricWindow {
  range: RangeKey;
  today: string;
  setRange: (range: RangeKey) => void;
  step: (direction: -1 | 1) => void;
}

function useMetricPeriod(def: MetricDef): MetricPeriod {
  const [params, setParams] = useSearchParams();
  const me = useMe();
  const today = todayIn(me.data?.timezone);
  const range = def.ranges.find((r) => r === params.get('range')) ?? def.defaultRange;
  const win = metricWindow(def.key, range, params.get('end'), today);
  const go = useCallback(
    (to: RangeKey, anchor: string) => {
      const next = new URLSearchParams({ range: to });
      if (anchor < today) next.set('end', anchor);
      setParams(next);
    },
    [setParams, today],
  );
  return {
    ...win,
    range,
    today,
    // A tab keeps the day the period is anchored on.
    setRange: (to) => go(to, win.anchor),
    step: (direction) => go(range, stepAnchor(range, win.anchor, direction, today)),
  };
}

const optionOf = (range: RangeKey): PeriodOption<RangeKey> => ({ value: range, label: range, days: RANGE_DAYS[range] });

export function MetricView({ metric }: { metric: MetricKey }) {
  const def = METRICS[metric];
  usePageTitle(def.title);
  const p = useMetricPeriod(def);
  const single = p.range === '1d';
  const q = useMetricRange(metric, single ? null : (p.range as ServerRange), p.anchor);
  const unit = single ? 'day' : metric === 'intensity_minutes' && p.range === '7d' ? 'week' : 'period';
  return (
    <>
      <TopBar variant="back" backTo="/" backLabel="Back to Today" title={def.title} aside={<SampleDataChip sample={q.data?.sample} />} />
      <RangeBar label="Period" options={def.ranges.map(optionOf)} value={p.range} onChange={p.setRange} />
      <div className="page" style={{ paddingTop: 0 }} aria-busy={single ? undefined : q.isPending}>
        <PeriodNav from={p.from} to={p.to} atToday={p.atToday} onStep={p.step} unit={unit} />
        {single ? (
          <DayBody metric={metric} date={p.to} today={p.today} />
        ) : q.isPending ? (
          <>
            <CardSkeleton lines={4} height={20} />
            <CardSkeleton lines={2} height={20} />
          </>
        ) : q.isError && !q.data ? (
          <LoadError what={`the ${def.title.toLowerCase()}`} error={q.error} onRetry={() => void q.refetch()} />
        ) : q.data ? (
          <div className="page-stack" style={{ opacity: q.isPlaceholderData ? 0.7 : 1 }}>
            <RangeBody def={def} data={q.data} today={p.today} />
          </div>
        ) : null}
      </div>
    </>
  );
}
