import { useMemo, useState } from 'react';
import type { ActivityDetail } from '../../api/generated';
import { LinkButton, MapView, SegmentedControl, Table, Unavailable, ZoneBars } from '../../components';
import { fmtClock, fmtFixed } from '../../lib/format';
import { zoneEntries, routeLabel } from './ActivityBody';
import { useTrack } from './api';
import { ChartBlock, DetailHeader, DetailLayout, SideCard, SideFacts, StatGroups, type Headline, type StatGroup } from './DetailParts';
import {
  activityTitle,
  DISTANCE_KINDS,
  fmtBpm,
  fmtDistance,
  fmtKcal,
  fmtMeters,
  fmtMinutes,
  fmtOne,
  fmtPace,
  fmtSpeed,
  fmtKg,
  longWhen,
} from './format';
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
import { kindNoun } from './DetailParts';
import { Tabs } from './Tabs';
import './activities.css';

const MAP_HEIGHT = 390;

/** "Treadmill" or "Trail": the watch's sub sport, when it says more than the sport does. */
function subSportText(a: Pick<ActivityDetail, 'sub_sport_name'>): string | null {
  const s = a.sub_sport_name?.replace(/_/g, ' ').trim();
  if (!s || s === 'generic') return null;
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/** The route map without a card around it; an activity without GPS says so in one line. */
function RouteMap({ a }: { a: ActivityDetail }) {
  const track = useTrack(a.id, a.has_track);
  // The API sends [lat, lon]; the map wants [lon, lat].
  const points = useMemo(() => track.data?.points.map(([lat, lon]) => [lon, lat] as [number, number]), [track.data]);
  if (!a.has_track) return <p className="adt-note">This activity has no GPS track.</p>;
  if (track.isError)
    return (
      <p role="alert" className="adt-note">
        The route could not be loaded.
      </p>
    );
  return (
    <div className="adt-map">
      <MapView label={routeLabel(a.distance_m)} track={points} height={MAP_HEIGHT} />
    </div>
  );
}

type Axis = 'time' | 'distance';

/** The chart stack, with a Time / Distance switch when the file has a distance for its samples. */
function Charts({ a }: { a: ActivityDetail }) {
  const s = a.series;
  const paced = a.kind === 'run' || a.kind === 'walk' || a.kind === 'hike';
  const distanceKind = DISTANCE_KINDS.has(a.kind);
  const [axis, setAxis] = useState<Axis>('time');

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

  return (
    <div className="adt-charts">
      <div className="adt-charts__bar">
        <h2 className="sr-only">Charts</h2>
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
      {distanceKind && hasAlt ? (
        <ChartBlock color="energy" title="Elevation" label="Elevation over the activity" summary={elevationSummary(alt, elapsed)} x={x} data={alt} xLabels={labelsFor(alt)} />
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
          {...(avgPaceMin !== null ? { refLine: { value: avgPaceMin, label: `Avg: ${clockFromMinutes(avgPaceMin)} /km` } } : {})}
        />
      ) : distanceKind && speedSpread !== null ? (
        <ChartBlock
          color="link"
          title="Speed"
          label="Speed over the activity"
          summary={speedSummary(speed, elapsed)}
          x={x}
          data={speed}
          xLabels={labelsFor(speed)}
          unit="decimal"
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
        {...(hrSpread !== null && avgHr !== null ? { refLine: { value: avgHr, label: `Avg: ${Math.round(avgHr)} bpm` } } : {})}
      />
      {cadSpread !== null ? (
        <ChartBlock
          color="warn"
          title="Cadence"
          label="Cadence over the activity"
          summary={cadenceSummary(cad, elapsed)}
          x={x}
          data={cad}
          xLabels={labelsFor(cad)}
          {...(avgCad !== null ? { refLine: { value: avgCad, label: `Avg: ${Math.round(avgCad)} /min` } } : {})}
        />
      ) : null}
    </div>
  );
}

/** The five figures under the title, by sport. */
export function headlineFor(a: ActivityDetail): Headline[] {
  const time = fmtClock(a.duration_s);
  const ascent = fmtMeters(a.ascent_m);
  const avgHr = fmtBpm(a.avg_hr_bpm);
  const base = { distance: fmtDistance(a.distance_m, 2), kcal: fmtKcal(a.calories_kcal) };
  switch (a.kind) {
    case 'run':
    case 'walk':
    case 'hike':
      return [
        { label: 'Distance', value: base.distance },
        { label: 'Time', value: time },
        { label: 'Avg pace', value: fmtPace(a.avg_pace_s_per_km) },
        { label: 'Total ascent', value: ascent },
        { label: 'Avg HR', value: avgHr },
      ];
    case 'cycle':
      return [
        { label: 'Distance', value: base.distance },
        { label: 'Time', value: time },
        { label: 'Avg speed', value: fmtSpeed(a.avg_speed_mps) },
        { label: 'Total ascent', value: ascent },
        { label: 'Avg HR', value: avgHr },
      ];
    case 'swim':
      return [
        { label: 'Distance', value: fmtDistance(a.distance_m, 1) },
        { label: 'Time', value: time },
        { label: 'Avg HR', value: avgHr },
        { label: 'Max HR', value: fmtBpm(a.max_hr_bpm) },
        { label: 'Calories', value: base.kcal },
      ];
    case 'strength':
      return [
        { label: 'Time', value: time },
        { label: 'Sets', value: a.sets === null ? null : String(a.sets) },
        { label: 'Volume', value: a.volume_kg === null ? null : fmtKg(Math.round(a.volume_kg)) },
        { label: 'Avg HR', value: avgHr },
        { label: 'Calories', value: base.kcal },
      ];
    default:
      return [
        { label: 'Time', value: time },
        { label: 'Distance', value: base.distance },
        { label: 'Avg HR', value: avgHr },
        { label: 'Max HR', value: fmtBpm(a.max_hr_bpm) },
        { label: 'Calories', value: base.kcal },
      ];
  }
}

/** The Stats tab: groups of figures the file has. */
export function statGroupsFor(a: ActivityDetail): StatGroup[] {
  const alt = a.series ? spread(a.series.altitude_m) : null;
  const paced = a.kind === 'run' || a.kind === 'walk' || a.kind === 'hike';
  const elapsedDiffers = a.elapsed_s !== null && a.duration_s !== null && Math.abs(a.elapsed_s - a.duration_s) >= 1;
  return [
    { title: 'Distance', items: [{ label: 'Distance', value: fmtDistance(a.distance_m, 2) }] },
    {
      title: 'Timing',
      items: [
        { label: 'Time', value: fmtClock(a.duration_s) },
        { label: 'Elapsed time', value: elapsedDiffers ? fmtClock(a.elapsed_s) : null },
      ],
    },
    {
      title: paced ? 'Pace' : 'Speed',
      items: [
        { label: 'Avg pace', value: paced ? fmtPace(a.avg_pace_s_per_km) : null },
        { label: 'Avg speed', value: fmtSpeed(a.avg_speed_mps) },
        { label: 'Max speed', value: fmtSpeed(a.max_speed_mps) },
      ],
    },
    {
      title: 'Heart rate',
      items: [
        { label: 'Avg HR', value: fmtBpm(a.avg_hr_bpm) },
        { label: 'Max HR', value: fmtBpm(a.max_hr_bpm) },
      ],
    },
    {
      title: 'Elevation',
      items: [
        { label: 'Total ascent', value: fmtMeters(a.ascent_m) },
        { label: 'Total descent', value: fmtMeters(a.descent_m) },
        { label: 'Min elev', value: alt ? fmtMeters(alt.min) : null },
        { label: 'Max elev', value: alt ? fmtMeters(alt.max) : null },
      ],
    },
    { title: 'Cadence', items: [{ label: 'Avg cadence', value: a.avg_cadence === null ? null : `${Math.round(a.avg_cadence)} /min` }] },
    {
      title: 'Strength',
      items: [
        { label: 'Sets', value: a.sets === null ? null : String(a.sets) },
        { label: 'Exercises', value: a.exercises === null ? null : String(a.exercises) },
        { label: 'Volume', value: a.volume_kg === null ? null : fmtKg(Math.round(a.volume_kg)) },
      ],
    },
    { title: 'Energy', items: [{ label: 'Calories', value: fmtKcal(a.calories_kcal) }] },
  ];
}

function LapsTable({ a }: { a: ActivityDetail }) {
  const paced = a.kind === 'run' || a.kind === 'walk' || a.kind === 'hike';
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
        { key: 'd', header: 'Distance', cell: (l) => fmtDistance(l.distance_m, 2) },
        { key: 't', header: 'Time', cell: (l) => fmtClock(l.duration_s) },
        ...(paced ? [{ key: 'p', header: 'Avg pace', cell: (l: ActivityDetail['laps'][number]) => (l.avg_pace_s_per_km === null ? null : fmtClock(l.avg_pace_s_per_km)) }] : []),
        { key: 'a', header: 'Ascent', cell: (l) => fmtMeters(l.ascent_m) },
        { key: 'hr', header: 'Avg HR', cell: (l) => (l.avg_hr_bpm === null ? null : Math.round(l.avg_hr_bpm)), align: 'end' },
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
  const effect = [
    { label: 'Aerobic effect', value: fmtOne(a.aerobic_te) },
    { label: 'Anaerobic effect', value: fmtOne(a.anaerobic_te) },
    { label: 'Training load', value: fmtFixed(a.training_load, 0) },
  ];
  const hasEffect = effect.some((e) => e.value !== null && e.value !== '');
  return (
    <>
      {a.has_strength || a.kind === 'strength' ? (
        <SideCard title="Strength">
          <LinkButton to={`/activities/${a.id}/strength`} variant="primary" size="lg">
            View strength sets
          </LinkButton>
        </SideCard>
      ) : null}
      {hasEffect ? (
        <SideCard title="Training effect">
          <SideFacts items={effect} />
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
        headline={headlineFor(a)}
      />
      {showMap ? <RouteMap a={a} /> : null}
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
