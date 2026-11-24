import { useMemo } from 'react';
import type { ActivityDetail } from '../../api/generated';
import { Card, CardHeader, MapView, SeriesChart, StatTile, Table, Unavailable, ZoneBars, type ZoneEntry } from '../../components';
import { LinkButton } from '../../components';
import { fmtFixed } from '../../lib/format';
import { useIsDesktop } from '../../lib/hooks';
import type { AxisUnit } from '../../lib/ticks';
import { RecordBadge } from '../records/RecordBadge';
import { useTrack } from './api';
import {
  DISTANCE_KINDS,
  fmtBpm,
  fmtDistance,
  fmtKcal,
  fmtMeters,
  fmtMinutes,
  fmtOne,
  fmtPace,
  fmtSpeed,
} from './format';
import { fmtClock } from '../../lib/format';
import {
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
import './activities.css';

/** Zones 1 to 5 in minutes; a zone the file does not list is unavailable, not 0. */
export function zoneEntries(a: Pick<ActivityDetail, 'hr_zones'>): ZoneEntry[] {
  return ([1, 2, 3, 4, 5] as const).map((zone) => {
    const z = a.hr_zones.find((x) => x.zone === zone);
    return { zone, minutes: z?.duration_s === null || z?.duration_s === undefined ? null : Math.round(z.duration_s / 60) };
  });
}

/** What the route map shows, in words (the drawing itself is not readable by a screen reader). */
export function routeLabel(distanceM: number | null): string {
  const length = distanceM !== null && distanceM > 0 ? `${(distanceM / 1000).toFixed(2)} km route` : 'route';
  return `Route map: the ${length} of this activity as a line from its start, marked with an orange dot. The numbers are in the stats and laps.`;
}

function MapCard({ id, hasTrack, height, distanceM }: { id: string; hasTrack: boolean; height: number; distanceM: number | null }) {
  const track = useTrack(id, hasTrack);
  // The API sends [lat, lon]; the map wants [lon, lat].
  const points = useMemo(() => track.data?.points.map(([lat, lon]) => [lon, lat] as [number, number]), [track.data]);
  if (!hasTrack) {
    return (
      <Card>
        <CardHeader title="Route" tone="muted" />
        <p>
          <Unavailable />
        </p>
        <p className="acts-note">This activity has no GPS track.</p>
      </Card>
    );
  }
  if (track.isError) {
    return (
      <Card>
        <CardHeader title="Route" tone="muted" />
        <p role="alert" className="acts-note">
          The route could not be loaded.
        </p>
      </Card>
    );
  }
  return (
    <Card padding="flush">
      <MapView label={routeLabel(distanceM)} track={points} height={height} />
    </Card>
  );
}

type Tile = { label: string; value: string | null; sub?: string };

/** StatTiles for the entries that have a value; the others are left out. */
function visibleTiles(tiles: (Tile | null)[], size: 'sm' | 'md') {
  return tiles
    .filter((t): t is Tile & { value: string } => t !== null && t.value !== null && t.value !== '')
    .map((t) => <StatTile key={t.label} size={size} label={t.label} value={t.value} {...(t.sub ? { sub: t.sub } : {})} />);
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
}) {
  return (
    <Card>
      <CardHeader title={title} tone="muted" aside={aside} />
      <SeriesChart
        label={label}
        summary={summary}
        x={x}
        series={[{ label: title, color, data, area: area }]}
        height={height}
        xLabels={spread(data) !== null ? timeLabels(x[x.length - 1] ?? 0) : []}
        {...(unit ? { yUnit: unit } : {})}
      />
    </Card>
  );
}

/** Everything below the top bar of an activity: map, stats, charts, zones, laps. */
export function ActivityBody({ a }: { a: ActivityDetail }) {
  const desktop = useIsDesktop();
  const distanceKind = DISTANCE_KINDS.has(a.kind);
  const paced = a.kind === 'run' || a.kind === 'walk' || a.kind === 'hike';
  const size = desktop ? 'md' : 'sm';
  const gridClass = desktop ? 'grid-tiles' : 'grid-3';
  const chartHeight = desktop ? 170 : 96;
  const showMap = a.has_track || a.kind === 'run' || a.kind === 'hike' || a.kind === 'cycle' || a.kind === 'walk';

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

  // A tile without a value is left out: "unavailable" on every missing stat made the page look empty.
  const speedOrPace = paced
    ? { label: 'Pace', value: fmtPace(a.avg_pace_s_per_km) }
    : a.kind === 'cycle'
      ? { label: 'Speed', value: fmtSpeed(a.avg_speed_mps) }
      : null;

  const primaryTiles: (Tile | null)[] = distanceKind
    ? [
        { label: 'Distance', value: fmtDistance(a.distance_m) },
        { label: 'Time', value: fmtClock(a.duration_s) },
        speedOrPace,
        { label: 'Avg HR', value: fmtBpm(a.avg_hr_bpm) },
        { label: 'Climb', value: fmtMeters(a.ascent_m) },
        { label: 'Energy', value: fmtKcal(a.calories_kcal) },
      ]
    : [
        { label: 'Time', value: fmtClock(a.duration_s) },
        { label: 'Avg HR', value: fmtBpm(a.avg_hr_bpm) },
        { label: 'Max HR', value: fmtBpm(a.max_hr_bpm) },
        { label: 'Energy', value: fmtKcal(a.calories_kcal) },
      ];
  const primary = visibleTiles(primaryTiles, size);

  const moreTiles: Tile[] = [
    { label: 'Max HR', value: distanceKind ? fmtBpm(a.max_hr_bpm) : null },
    { label: 'Descent', value: fmtMeters(a.descent_m) },
    { label: 'Cadence', value: a.avg_cadence === null ? null : `${Math.round(a.avg_cadence)} /min` },
    { label: 'Aerobic effect', value: fmtOne(a.aerobic_te) },
    { label: 'Anaerobic effect', value: fmtOne(a.anaerobic_te) },
    { label: 'Training load', value: fmtFixed(a.training_load, 0) },
  ];
  const more = visibleTiles(moreTiles, size);

  const zones = zoneEntries(a);
  const belowZone1 = fmtMinutes(a.hr_below_zone1_s);
  const lapPaced = paced;

  return (
    <div className="page" data-activity-id={a.id}>
      <RecordBadge kinds={a.records} />
      {a.has_strength || a.kind === 'strength' ? (
        <Card as="div" padding="default">
          <LinkButton to={`/activities/${a.id}/strength`} variant="primary" size="lg">
            View strength sets
          </LinkButton>
        </Card>
      ) : null}

      {desktop ? (
        <div className="acts-panels">
          {showMap ? <MapCard id={a.id} hasTrack={a.has_track} height={300} distanceM={a.distance_m} /> : null}
          <div className="acts-stack">
            {primary.length ? <div className={gridClass}>{primary}</div> : null}
          </div>
        </div>
      ) : (
        <>
          {showMap ? <MapCard id={a.id} hasTrack={a.has_track} height={220} distanceM={a.distance_m} /> : null}
          {primary.length ? <div className={gridClass}>{primary}</div> : null}
        </>
      )}

      <div className="acts-stack">
        <ChartCard
          title="Heart rate"
          aside={hrSpread ? `Avg ${Math.round(hrSpread.avg)} · Max ${Math.round(hrSpread.max)} bpm` : null}
          label="Heart rate over the activity"
          summary={hrSummary(s)}
          x={x}
          data={hr}
          color="heart"
          height={chartHeight}
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
            height={chartHeight}
            unit="pace"
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
            height={chartHeight}
            unit="decimal"
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
            height={chartHeight}
            area
          />
        ) : null}
      </div>

      <div className={desktop ? 'acts-panels' : 'acts-stack'}>
        <Card>
          <CardHeader title="Time in heart rate zones" tone="muted" />
          <ZoneBars zones={zones} />
          {belowZone1 ? <p className="acts-note">Below zone 1: {belowZone1}</p> : null}
        </Card>
        <div className="acts-stack">
          {more.length ? <div className={gridClass}>{more}</div> : null}
          {/* Full width: its note needs the room a third of the row does not give. */}
          {a.vo2max_derived !== null ? (
            <StatTile
              size={size}
              label="VO2 max (derived)"
              value={`${a.vo2max_derived.toFixed(1)} ml/kg/min`}
              sub="Estimated from this activity (maximal MET x 3.5), not Garmin's published value."
            />
          ) : null}
        </div>
      </div>

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
              lapPaced
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
