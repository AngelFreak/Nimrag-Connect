import { useMemo, useState } from 'react';
import type { ActivityDetail } from '../../api/generated';
import { LinkButton, SegmentedControl, Table, Unavailable, ZoneBars, useCursorStore } from '../../components';
import { fmtClock } from '../../lib/format';
import { zoneEntries } from './ActivityBody';
import { ChartReadout, type ReadoutField } from './ChartReadout';
import { ChartBlock, DetailHeader, DetailLayout, SideCard, SideFacts, StatGroups, kindNoun } from './DetailParts';
import { activityTitle, DISTANCE_KINDS, fmtDistance, fmtMeters, fmtMinutes, longWhen } from './format';
import { zoneLimits, zoneOf } from './lapZones';
import { RouteExplorer } from './RouteExplorer';
import {
  cadenceSummary,
  clockFromMinutes,
  distanceAxis,
  distanceLabels,
  elevationSummary,
  hrSummary,
  minutesAxis,
  paceMinutes,
  paceSummary,
  speedKmh,
  speedSummary,
  spread,
  timeLabels,
} from './series';
import { isPaced, shownHeadline, statGroupsFor, subSportText } from './summary';
import { Tabs } from './Tabs';
import { TrainingEffect, hasTrainingEffect } from './TrainingEffect';
import { RecordBadge } from '../records/RecordBadge';
import { cadenceUnit, stepsCadence } from './cadence';
import './activities.css';

const MAP_HEIGHT = 390;

type Axis = 'time' | 'distance';

/** The chart stack, with a Time / Distance switch when the file has a distance for its samples, and one pointer line over all of it. */
function Charts({ a }: { a: ActivityDetail }) {
  const s = a.series;
  const paced = isPaced(a.kind);
  const distanceKind = DISTANCE_KINDS.has(a.kind);
  const [axis, setAxis] = useState<Axis>('time');
  const store = useCursorStore();
  const cursor = { group: 'activity', store };

  const minutes = useMemo(() => (s ? minutesAxis(s.elapsed_s) : []), [s]);
  const km = useMemo(() => (s ? distanceAxis(s.distance_m) : null), [s]);
  const useDistance = axis === 'distance' && km !== null;
  const x = useDistance ? (km as number[]) : minutes;
  const xLabels = useMemo(() => (useDistance ? distanceLabels(x[x.length - 1] ?? 0) : timeLabels(x[x.length - 1] ?? 0)), [useDistance, x]);

  const pace = useMemo(() => (s ? paceMinutes(s.speed_mps) : []), [s]);
  const speed = useMemo(() => (s ? speedKmh(s.speed_mps) : []), [s]);
  const hr = s?.heart_rate_bpm ?? [];
  const alt = s?.altitude_m ?? [];
  const cad = s?.cadence ?? [];
  const elapsed = s?.elapsed_s ?? [];
  const hrSpread = spread(hr);
  const paceSpread = spread(pace);
  const speedSpread = spread(speed);
  const cadSpread = spread(cad);
  const hasAlt = spread(alt) !== null;
  const labelsFor = (data: (number | null)[]) => (spread(data) !== null ? xLabels : []);

  const avgPaceMin = a.avg_pace_s_per_km !== null ? a.avg_pace_s_per_km / 60 : (paceSpread?.avg ?? null);
  const avgHr = a.avg_hr_bpm ?? hrSpread?.avg ?? null;
  const avgSpeed = a.avg_speed_mps !== null ? a.avg_speed_mps * 3.6 : (speedSpread?.avg ?? null);
  const avgCad = a.avg_cadence ?? cadSpread?.avg ?? null;

  // The charts drawn, in order: the readout lists the same.
  const showAlt = distanceKind && hasAlt;
  const showSpeed = !paced && distanceKind && speedSpread !== null;
  const fields: ReadoutField[] = [
    ...(showAlt ? (['elevation'] as const) : []),
    ...(paced ? (['pace'] as const) : showSpeed ? (['speed'] as const) : []),
    ...(hrSpread !== null ? (['hr'] as const) : []),
    ...(cadSpread !== null ? (['cadence'] as const) : []),
  ];

  return (
    <div className="adt-charts">
      <div className="adt-charts__bar">
        <h2 className="sr-only">Charts</h2>
        {fields.length ? <ChartReadout store={store} series={s} fields={fields} cadenceUnit={cadenceUnit(a.kind)} /> : null}
        {km !== null ? (
          <SegmentedControl<Axis>
            label="Chart x axis"
            options={[
              { value: 'time', label: 'Time' },
              { value: 'distance', label: 'Distance' },
            ]}
            value={axis}
            onChange={setAxis}
          />
        ) : null}
      </div>
      {showAlt ? (
        <ChartBlock color="energy" title="Elevation" label="Elevation over the activity" summary={elevationSummary(alt, elapsed)} x={x} data={alt} xLabels={labelsFor(alt)} cursor={cursor} />
      ) : null}
      {paced ? (
        <ChartBlock
          color="link"
          title="Pace"
          label="Pace over the activity"
          summary={paceSummary(pace, elapsed)}
          x={x}
          data={pace}
          xLabels={labelsFor(pace)}
          unit="pace"
          yReverse
          cursor={cursor}
          {...(avgPaceMin !== null ? { refLine: { value: avgPaceMin, label: `Avg: ${clockFromMinutes(avgPaceMin)} /km` } } : {})}
        />
      ) : showSpeed ? (
        <ChartBlock
          color="link"
          title="Speed"
          label="Speed over the activity"
          summary={speedSummary(speed, elapsed)}
          x={x}
          data={speed}
          xLabels={labelsFor(speed)}
          unit="decimal"
          cursor={cursor}
          {...(avgSpeed !== null ? { refLine: { value: avgSpeed, label: `Avg: ${avgSpeed.toFixed(1)} km/h` } } : {})}
        />
      ) : null}
      <ChartBlock
        color="heart"
        title="Heart rate"
        label="Heart rate over the activity"
        summary={hrSummary(s)}
        x={x}
        data={hr}
        xLabels={labelsFor(hr)}
        cursor={cursor}
        {...(hrSpread !== null && avgHr !== null ? { refLine: { value: avgHr, label: `Avg: ${Math.round(avgHr)} bpm` } } : {})}
      />
      {cadSpread !== null ? (
        <ChartBlock
          color="warn"
          title="Cadence"
          label="Cadence over the activity"
          summary={cadenceSummary(cad, elapsed, stepsCadence(a.kind) ? 'steps a minute' : 'revolutions a minute')}
          x={x}
          data={cad}
          xLabels={labelsFor(cad)}
          cursor={cursor}
          {...(avgCad !== null ? { refLine: { value: avgCad, label: `Avg: ${Math.round(avgCad)} ${cadenceUnit(a.kind)}` } } : {})}
        />
      ) : null}
    </div>
  );
}

function LapsTable({ a }: { a: ActivityDetail }) {
  const paced = isPaced(a.kind);
  const limits = useMemo(() => zoneLimits(a.series?.heart_rate_bpm ?? [], a.hr_zones, a.hr_below_zone1_s), [a.series, a.hr_zones, a.hr_below_zone1_s]);
  if (a.laps.length === 0) {
    return (
      <p>
        <Unavailable />
      </p>
    );
  }
  return (
    <Table
      caption="Laps"
      density="compact"
      rows={a.laps}
      rowKey={(l) => String(l.index)}
      columns={[
        { key: 'n', header: 'Lap', cell: (l) => l.index },
        { key: 'd', header: 'Distance', cell: (l) => fmtDistance(l.distance_m) },
        { key: 't', header: 'Time', cell: (l) => fmtClock(l.duration_s) },
        ...(paced ? [{ key: 'p', header: 'Avg pace', cell: (l: ActivityDetail['laps'][number]) => (l.avg_pace_s_per_km === null ? null : fmtClock(l.avg_pace_s_per_km)) }] : []),
        { key: 'a', header: 'Ascent', cell: (l) => fmtMeters(l.ascent_m) },
        {
          key: 'hr',
          header: 'Avg HR',
          align: 'end',
          cell: (l) => {
            if (l.avg_hr_bpm === null) return null;
            const zone = limits ? zoneOf(l.avg_hr_bpm, limits) : null;
            return (
              <span className="lap-hr">
                {zone !== null ? (
                  <>
                    <span className={`lap-hr__dot${zone === 0 ? ' lap-hr__dot--below' : ''}`} style={zone > 0 ? { background: `var(--zone-${zone})` } : undefined} aria-hidden="true" />
                    <span className="sr-only">{zone === 0 ? 'Below zone 1: ' : `Zone ${zone}: `}</span>
                  </>
                ) : null}
                {Math.round(l.avg_hr_bpm)}
              </span>
            );
          },
        },
      ]}
    />
  );
}

function ZonesPanel({ a }: { a: ActivityDetail }) {
  const below = fmtMinutes(a.hr_below_zone1_s);
  return (
    <div className="adt-zones">
      <ZoneBars zones={zoneEntries(a)} />
      {below ? <p className="adt-note">Below zone 1: {below}</p> : null}
    </div>
  );
}

function SideColumn({ a }: { a: ActivityDetail }) {
  return (
    <>
      {a.has_strength || a.kind === 'strength' ? (
        <SideCard title="Strength">
          <LinkButton to={`/activities/${a.id}/strength`} variant="primary" size="lg">
            View strength sets
          </LinkButton>
        </SideCard>
      ) : null}
      {hasTrainingEffect(a) ? (
        <SideCard title="Training effect">
          <TrainingEffect a={a} />
        </SideCard>
      ) : null}
      {a.vo2max_derived !== null ? (
        <SideCard title="Fitness estimate">
          <SideFacts
            items={[
              {
                label: 'VO2 max (derived)',
                value: `${a.vo2max_derived.toFixed(1)} ml/kg/min`,
                sub: "Estimated from this activity (maximal MET x 3.5), not Garmin's published value.",
              },
            ]}
          />
        </SideCard>
      ) : null}
      <SideCard title="Details">
        <SideFacts
          items={[
            { label: 'Type', value: [activityTitle(a), subSportText(a)].filter(Boolean).join(' · ') },
            { label: 'Started', value: longWhen(a) },
          ]}
        />
      </SideCard>
    </>
  );
}

/** An activity on a wide screen: breadcrumb, title, headline figures, map, charts and tabs, with a side column. */
export function ActivityDetailDesktop({ a }: { a: ActivityDetail }) {
  const showMap = a.has_track || a.kind === 'run' || a.kind === 'hike' || a.kind === 'cycle' || a.kind === 'walk';
  const backTo = a.kind === 'strength' ? '/strength' : '/activities';
  const main = (
    <>
      <DetailHeader
        backTo={backTo}
        section={a.kind === 'strength' ? 'Strength' : 'Activities'}
        noun={kindNoun(a)}
        when={longWhen(a)}
        kind={a.kind}
        title={activityTitle(a)}
        meta={subSportText(a)}
        sample={a.sample}
        headline={shownHeadline(a)}
        badge={a.records.length > 0 ? <RecordBadge kinds={a.records} /> : null}
      />
      {showMap ? <RouteExplorer a={a} height={MAP_HEIGHT} variant="bare" /> : null}
      <Charts a={a} />
      <Tabs
        label="Activity details"
        tabs={[
          { id: 'stats', label: 'Stats', content: <StatGroups groups={statGroupsFor(a)} /> },
          { id: 'laps', label: 'Laps', content: <LapsTable a={a} /> },
          { id: 'zones', label: 'Time in Zones', content: <ZonesPanel a={a} /> },
        ]}
      />
    </>
  );
  return (
    <div className="adt-root" data-activity-id={a.id}>
      <DetailLayout main={main} side={<SideColumn a={a} />} />
    </div>
  );
}
