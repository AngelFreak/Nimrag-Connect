import { useMemo, useState } from 'react';
import type { ActivityDetail } from '../../api/generated';
import { Card, CardHeader, MapView, SegmentedControl, Unavailable } from '../../components';
import { useTrack } from './api';
import {
  ROUTE_CHOICE_LABELS,
  defaultRouteChoice,
  metricValues,
  rampGradient,
  routeChoices,
  routeColors,
  routeLegend,
  routeScale,
  type RouteChoice,
  type RouteLegendText,
} from './routeColor';
import './activities.css';

/** What the route map shows, in words (the drawing itself is not readable by a screen reader). */
export function routeLabel(distanceM: number | null, colouredBy?: string): string {
  const length = distanceM !== null && distanceM > 0 ? `${(distanceM / 1000).toFixed(2)} km route` : 'route';
  const coloured = colouredBy ? ` The line is coloured by ${colouredBy.toLowerCase()}.` : '';
  return `Route map: the ${length} of this activity as a line from its start, marked with an orange dot.${coloured} The numbers are in the stats and laps.`;
}

/** The key under the map: the ramp with the value at each end. The row keeps its height while the line is plain. */
function Legend({ legend }: { legend: RouteLegendText | null }) {
  return (
    <div className="route__legend">
      {legend ? (
        <>
          <span className="route__end" aria-hidden="true">
            {legend.low}
          </span>
          <span className="route__ramp" aria-hidden="true" style={{ background: rampGradient() }} />
          <span className="route__end" aria-hidden="true">
            {legend.high}
          </span>
          <span className="sr-only">{`Route colour: ${legend.what}, from ${legend.low} (cool colour) to ${legend.high} (warm colour).`}</span>
        </>
      ) : null}
    </div>
  );
}

interface RouteExplorerProps {
  a: ActivityDetail;
  height: number;
  /** "card" is the phone page (a card with a header when there is nothing to draw); "bare" is the desktop page. */
  variant: 'card' | 'bare';
}

/**
 * The route of an activity: a switch for what colours the line (pace, speed, heart rate, elevation or
 * nothing), the map, and the key under it. A sport without GPS says so in one line.
 */
export function RouteExplorer({ a, height, variant }: RouteExplorerProps) {
  const track = useTrack(a.id, a.has_track);
  const choices = useMemo(() => routeChoices(a.kind, a.series), [a.kind, a.series]);
  const [picked, setPicked] = useState<RouteChoice | null>(null);
  const choice = picked && choices.includes(picked) ? picked : defaultRouteChoice(a.kind, choices);

  // The API sends [lat, lon]; the map wants [lon, lat].
  const points = useMemo(() => track.data?.points.map(([lat, lon]) => [lon, lat] as [number, number]), [track.data]);
  const coloring = useMemo(() => {
    if (choice === 'plain' || !track.data || !points) return null;
    const values = metricValues(choice, track.data);
    const scale = values.length === points.length ? routeScale(values) : null;
    return scale ? { colors: routeColors(values, scale), legend: routeLegend(choice, scale) } : null;
  }, [choice, track.data, points]);

  if (!a.has_track) {
    return variant === 'card' ? (
      <Card>
        <CardHeader title="Route" tone="muted" />
        <p>
          <Unavailable />
        </p>
        <p className="acts-note">This activity has no GPS track.</p>
      </Card>
    ) : (
      <p className="adt-note">This activity has no GPS track.</p>
    );
  }
  if (track.isError) {
    return variant === 'card' ? (
      <Card>
        <CardHeader title="Route" tone="muted" />
        <p role="alert" className="acts-note">
          The route could not be loaded.
        </p>
      </Card>
    ) : (
      <p role="alert" className="adt-note">
        The route could not be loaded.
      </p>
    );
  }

  const switchable = choices.length > 1;
  const inner = (
    <>
      {switchable ? (
        <div className="route__bar">
          <SegmentedControl<RouteChoice>
            label="Colour the route by"
            options={choices.map((c) => ({ value: c, label: ROUTE_CHOICE_LABELS[c] }))}
            value={choice}
            onChange={setPicked}
          />
        </div>
      ) : null}
      <MapView label={routeLabel(a.distance_m, coloring?.legend.what)} track={points} height={height} routeColors={coloring?.colors ?? null} />
      {switchable ? <Legend legend={coloring?.legend ?? null} /> : null}
    </>
  );
  return variant === 'card' ? (
    <Card padding="flush" className="route route--card">
      {inner}
    </Card>
  ) : (
    <div className="adt-map route">{inner}</div>
  );
}
