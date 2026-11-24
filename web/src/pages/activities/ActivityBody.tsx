import { useMemo } from 'react';
import type { ActivityDetail } from '../../api/generated';
import { Card, CardHeader, LinkButton, SeriesChart, StatTile, Table, Unavailable, ZoneBars, useCursorStore, type CursorStore, type ZoneEntry } from '../../components';
import type { AxisUnit } from '../../lib/ticks';
import { RecordBadge } from '../records/RecordBadge';
import { fmtClock } from '../../lib/format';
import { ReadoutAside, type ReadoutField } from './ChartReadout';
import { DISTANCE_KINDS, fmtDistance, fmtMinutes } from './format';
import { RouteExplorer } from './RouteExplorer';
import { SportDisc } from './SportDisc';
import { StatGroups } from './DetailParts';
import { TrainingEffect, hasTrainingEffect } from './TrainingEffect';
import { elevationSummary, hrSummary, minutesAxis, paceMinutes, paceSummary, speedKmh, speedSummary, spread, timeLabels } from './series';
import { isPaced, shownHeadline, statGroupsFor } from './summary';
import './activities.css';

/** Zones 1 to 5 in minutes; a zone the file does not list is unavailable, not 0. */
export function zoneEntries(a: Pick<ActivityDetail, 'hr_zones'>): ZoneEntry[] {
  return ([1, 2, 3, 4, 5] as const).map((zone) => {
    const z = a.hr_zones.find((x) => x.zone === zone);
    return { zone, minutes: z?.duration_s === null || z?.duration_s === undefined ? null : Math.round(z.duration_s / 60) };
  });
}

/** The top of the page: the sport, the name and time, and the key figures of the sport in large type. */
function Hero({ a }: { a: ActivityDetail }) {
  const cells = shownHeadline(a);
  return (
    <section className="card acts-hero" aria-label="Summary">
      {/* The page header above already says the name and the time; this card is the icon and the figures. */}
      <SportDisc kind={a.kind} size={44} tone="soft" />
      {cells.length ? (
        <dl className="acts-hero__stats" data-count={cells.length}>
          {cells.map((c) => (
            <div key={c.label} className="acts-hero__stat">
              <dt>{c.label}</dt>
              <dd>{c.value}</dd>
            </div>
          ))}
        </dl>
      ) : null}
    </section>
  );
}

function ChartCard({
  title,
  aside,
  label,
  summary,
  x,
  data,
  color,
  height,
  unit,
  area,
  store,
  series,
  field,
}: {
  title: string;
  aside?: string | null;
  label: string;
  summary: string;
  x: number[];
  data: (number | null)[];
  color: 'heart' | 'link' | 'energy' | 'steps';
  height: number;
  unit?: AxisUnit;
  area?: boolean;
  store: CursorStore;
  series: ActivityDetail['series'];
  field: ReadoutField;
}) {
  return (
    <Card>
      <CardHeader title={title} tone="muted" aside={<ReadoutAside store={store} series={series} field={field} fallback={aside} />} />
      <SeriesChart
        label={label}
        summary={summary}
        x={x}
        series={[{ label: title, color, data, area: area }]}
        height={height}
        xLabels={spread(data) !== null ? timeLabels(x[x.length - 1] ?? 0) : []}
        cursor={{ group: 'activity', store }}
        {...(unit ? { yUnit: unit } : {})}
      />
    </Card>
  );
}

/** Everything below the top bar of an activity on a phone: summary, map, charts, effect, zones, stats, laps. */
export function ActivityBody({ a }: { a: ActivityDetail }) {
  const distanceKind = DISTANCE_KINDS.has(a.kind);
  const paced = isPaced(a.kind);
  const showMap = a.has_track || a.kind === 'run' || a.kind === 'hike' || a.kind === 'cycle' || a.kind === 'walk';
  const store = useCursorStore();

  const s = a.series;
  const x = useMemo(() => (s ? minutesAxis(s.elapsed_s) : []), [s]);
  const pace = useMemo(() => (s ? paceMinutes(s.speed_mps) : []), [s]);
  const speed = useMemo(() => (s ? speedKmh(s.speed_mps) : []), [s]);
  const hr = s?.heart_rate_bpm ?? [];
  const alt = s?.altitude_m ?? [];
  const hrSpread = spread(hr);
  const hasAlt = spread(alt) !== null;
  const hasSpeed = spread(speed) !== null;
  const elapsed = s?.elapsed_s ?? [];

  const groups = statGroupsFor(a);
  const zones = zoneEntries(a);
  const belowZone1 = fmtMinutes(a.hr_below_zone1_s);

  return (
    <div className="page" data-activity-id={a.id}>
      <RecordBadge kinds={a.records} />
      <Hero a={a} />

      {a.has_strength || a.kind === 'strength' ? (
        <Card as="div" padding="default">
          <LinkButton to={`/activities/${a.id}/strength`} variant="primary" size="lg">
            View strength sets
          </LinkButton>
        </Card>
      ) : null}

      {showMap ? <RouteExplorer a={a} height={220} variant="card" /> : null}

      <div className="acts-stack">
        <ChartCard
          title="Heart rate"
          aside={hrSpread ? `Avg ${Math.round(hrSpread.avg)} · Max ${Math.round(hrSpread.max)} bpm` : null}
          label="Heart rate over the activity"
          summary={hrSummary(s)}
          x={x}
          data={hr}
          color="heart"
          height={96}
          store={store}
          series={s}
          field="hr"
        />
        {paced ? (
          <ChartCard
            title="Pace"
            aside="Minutes per km, lower is faster"
            label="Pace over the activity"
            summary={paceSummary(pace, elapsed)}
            x={x}
            data={pace}
            color="link"
            height={96}
            unit="pace"
            store={store}
            series={s}
            field="pace"
          />
        ) : hasSpeed && distanceKind ? (
          <ChartCard
            title="Speed"
            aside="km/h"
            label="Speed over the activity"
            summary={speedSummary(speed, elapsed)}
            x={x}
            data={speed}
            color="link"
            height={96}
            unit="decimal"
            store={store}
            series={s}
            field="speed"
          />
        ) : null}
        {distanceKind ? (
          <ChartCard
            title="Elevation"
            aside={hasAlt ? 'Metres' : null}
            label="Elevation over the activity"
            summary={elevationSummary(alt, elapsed)}
            x={x}
            data={alt}
            color="energy"
            height={96}
            area
            store={store}
            series={s}
            field="elevation"
          />
        ) : null}
      </div>

      {hasTrainingEffect(a) ? (
        <Card>
          <CardHeader title="Training effect" tone="muted" />
          <TrainingEffect a={a} />
        </Card>
      ) : null}

      <Card>
        <CardHeader title="Time in heart rate zones" tone="muted" />
        <ZoneBars zones={zones} />
        {belowZone1 ? <p className="acts-note">Below zone 1: {belowZone1}</p> : null}
      </Card>

      {groups.length ? (
        <Card className="acts-stats">
          <CardHeader title="Stats" tone="muted" />
          <StatGroups groups={groups} />
        </Card>
      ) : null}

      {/* Full width: its note needs the room a third of the row does not give. */}
      {a.vo2max_derived !== null ? (
        <StatTile
          size="sm"
          label="VO2 max (derived)"
          value={`${a.vo2max_derived.toFixed(1)} ml/kg/min`}
          sub="Estimated from this activity (maximal MET x 3.5), not Garmin's published value."
        />
      ) : null}

      <Card>
        <CardHeader title="Laps" tone="muted" />
        {a.laps.length === 0 ? (
          <p>
            <Unavailable />
          </p>
        ) : (
          <Table
            caption="Laps"
            density="compact"
            rows={a.laps}
            rowKey={(l) => String(l.index)}
            columns={[
              { key: 'n', header: 'Lap', cell: (l) => l.index },
              { key: 'd', header: 'Distance', cell: (l) => fmtDistance(l.distance_m) },
              paced
                ? { key: 'p', header: 'Pace', cell: (l) => (l.avg_pace_s_per_km === null ? null : fmtClock(l.avg_pace_s_per_km)) }
                : { key: 't', header: 'Time', cell: (l) => fmtClock(l.duration_s) },
              { key: 'hr', header: 'HR', cell: (l) => (l.avg_hr_bpm === null ? null : Math.round(l.avg_hr_bpm)), align: 'end' },
            ]}
          />
        )}
      </Card>
    </div>
  );
}
