import { useEffect, useRef, useState } from 'react';
import type { GeoJSONSource, Map as MapLibreMap, MapOptions } from 'maplibre-gl';
import { colorRuns } from '../lib/colorRuns';
import { useMapChoice } from '../lib/mapPref';
import { useTheme, type Theme } from '../lib/theme';

// Maps use MapLibre GL JS with PMTiles from the owner's own server (/tiles/*.pmtiles).
// There is no third-party tile server and no hosted style or glyph URL: when the tile file
// is missing the route is drawn on its own instead. MapLibre itself is loaded lazily,
// so pages without a map do not pay for it.

/** Default tile file on the owner's server. Override with VITE_TILES_URL at build time. */
export const DEFAULT_TILES_URL: string = import.meta.env['VITE_TILES_URL'] ?? '/tiles/map.pmtiles';

type MapStyle = Exclude<MapOptions['style'], string | undefined>;

const probes = new Map<string, Promise<boolean>>();

/** True when the server really serves a PMTiles file (not the SPA's index.html, not a 404). */
export function tilesAvailable(url: string): Promise<boolean> {
  let p = probes.get(url);
  if (!p) {
    p = fetch(url, { headers: { Range: 'bytes=0-15' }, credentials: 'same-origin' })
      .then((res) => (res.status === 200 || res.status === 206) && !(res.headers.get('content-type') ?? '').includes('text/html'))
      .catch(() => false);
    probes.set(url, p);
  }
  return p;
}

type MapLayers = MapStyle['layers'];

/** Land use that reads as green on the light map (Protomaps `pmap:kind`); a tile without the property simply gets the plain fill. */
const GREEN_KINDS = ['park', 'nature_reserve', 'forest', 'wood', 'grass', 'grassland', 'golf_course', 'cemetery', 'garden', 'recreation_ground', 'scrub', 'meadow'];

const ROAD_WIDTH = (a: number, b: number, c: number): ['interpolate', ['linear'], ['zoom'], number, number, number, number, number, number] => [
  'interpolate',
  ['linear'],
  ['zoom'],
  8,
  a,
  14,
  b,
  18,
  c,
];

function darkLayers(): MapLayers {
  return [
    { id: 'background', type: 'background', paint: { 'background-color': '#141b1f' } },
    { id: 'earth', type: 'fill', source: 'base', 'source-layer': 'earth', paint: { 'fill-color': '#182126' } },
    { id: 'landuse', type: 'fill', source: 'base', 'source-layer': 'landuse', paint: { 'fill-color': '#1a2428' } },
    { id: 'water', type: 'fill', source: 'base', 'source-layer': 'water', paint: { 'fill-color': '#0f2230' } },
    { id: 'buildings', type: 'fill', source: 'base', 'source-layer': 'buildings', minzoom: 14, paint: { 'fill-color': '#1c262b' } },
    {
      id: 'roads',
      type: 'line',
      source: 'base',
      'source-layer': 'roads',
      layout: { 'line-cap': 'round', 'line-join': 'round' },
      paint: { 'line-color': '#2b373d', 'line-width': ROAD_WIDTH(0.5, 2, 8) },
    },
  ];
}

/** Light grey map in Connect's manner: pale land, green parks, blue water, white roads with a thin edge. */
function lightLayers(): MapLayers {
  const green = ['in', ['get', 'pmap:kind'], ['literal', GREEN_KINDS]] as const;
  return [
    { id: 'background', type: 'background', paint: { 'background-color': '#e8ecee' } },
    { id: 'earth', type: 'fill', source: 'base', 'source-layer': 'earth', paint: { 'fill-color': '#f1f2f0' } },
    { id: 'landuse', type: 'fill', source: 'base', 'source-layer': 'landuse', paint: { 'fill-color': '#ebeeea' } },
    { id: 'landuse-green', type: 'fill', source: 'base', 'source-layer': 'landuse', filter: green as never, paint: { 'fill-color': '#d8e7cf' } },
    { id: 'natural-green', type: 'fill', source: 'base', 'source-layer': 'natural', filter: green as never, paint: { 'fill-color': '#d8e7cf' } },
    { id: 'water', type: 'fill', source: 'base', 'source-layer': 'water', paint: { 'fill-color': '#bcd6ea' } },
    { id: 'buildings', type: 'fill', source: 'base', 'source-layer': 'buildings', minzoom: 14, paint: { 'fill-color': '#dfe1e2' } },
    {
      id: 'roads-edge',
      type: 'line',
      source: 'base',
      'source-layer': 'roads',
      layout: { 'line-cap': 'round', 'line-join': 'round' },
      paint: { 'line-color': '#cdd1d4', 'line-width': ROAD_WIDTH(0.8, 3, 10) },
    },
    {
      id: 'roads',
      type: 'line',
      source: 'base',
      'source-layer': 'roads',
      layout: { 'line-cap': 'round', 'line-join': 'round' },
      paint: { 'line-color': '#ffffff', 'line-width': ROAD_WIDTH(0.4, 2, 8) },
    },
  ];
}

/** Base style for the Protomaps vector schema, without labels (no glyph server needed): dark by default, light on request. */
export function buildStyle(pmtilesUrl: string, theme: Theme = 'dark'): MapStyle {
  const absolute = new URL(pmtilesUrl, window.location.origin).href;
  return {
    version: 8,
    sources: {
      base: { type: 'vector', url: `pmtiles://${absolute}`, attribution: '© OpenStreetMap contributors' },
    },
    layers: theme === 'light' ? lightLayers() : darkLayers(),
  };
}

/** The route, start-dot and finish-ring colours on each base map. */
const ROUTE = {
  dark: { line: '#7dbbff', start: '#ffb066', background: '#141b1f', finishFill: '#141b1f', finishRing: '#e8eef0' },
  light: { line: '#1265c2', start: '#e8680c', background: '#e8ecee', finishFill: '#ffffff', finishRing: '#1c2a33' },
} as const;

/** The line under the route: white on the light map (clear of white roads), dark under a coloured route (any colour of the ramp stays readable). */
export function casingFor(theme: Theme, coloured: boolean): { color: string; width: number; opacity: number } {
  if (coloured) return theme === 'light' ? { color: '#27363f', width: 7.5, opacity: 1 } : { color: '#05090b', width: 7.5, opacity: 1 };
  return theme === 'light' ? { color: '#ffffff', width: 7, opacity: 1 } : { color: '#ffffff', width: 0, opacity: 0 };
}

interface RouteFeature {
  type: 'Feature';
  properties: { c: string };
  geometry: { type: 'LineString'; coordinates: [number, number][] };
}

/** Route as GeoJSON for the line layer: one feature, or one per run of one colour. Every feature carries its colour as `c`. */
export function routeCollection(track: [number, number][], colors: readonly string[] | null | undefined, plain: string): { type: 'FeatureCollection'; features: RouteFeature[] } {
  const feature = (coordinates: [number, number][], c: string): RouteFeature => ({ type: 'Feature', properties: { c }, geometry: { type: 'LineString', coordinates } });
  if (!colors || colors.length !== track.length) return { type: 'FeatureCollection', features: [feature(track, plain)] };
  return { type: 'FeatureCollection', features: colorRuns(colors).map((r) => feature(track.slice(r.from, r.to + 1), r.color)) };
}

interface MapViewProps {
  /** Route as [longitude, latitude] pairs. */
  track?: [number, number][];
  tilesUrl?: string;
  /** Accessible name and summary of the route. The map is a group (not an image) because it holds MapLibre's zoom buttons; the stats beside it are what a screen reader reads. */
  label?: string;
  height?: number;
  /** One colour per point of `track` (a coloured route); without it the route is one colour. */
  routeColors?: readonly string[] | null;
}

/**
 * The route drawn on its own, without a base map, for when the server has no tile file. Longitude
 * is scaled by cos(latitude) so the shape is not stretched; with no track there is no drawing.
 */
export function routePath(track: [number, number][] | undefined, width: number, height: number, pad = 12): { d: string; start: [number, number]; end: [number, number]; at: (i: number) => [number, number] } | null {
  if (!track || track.length < 2) return null;
  const lats = track.map((p) => p[1]);
  const midLat = ((Math.min(...lats) + Math.max(...lats)) / 2) * (Math.PI / 180);
  const xs = track.map((p) => p[0] * Math.cos(midLat));
  const ys = lats.map((lat) => -lat);
  const [minX, maxX, minY, maxY] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  const scale = Math.min((width - 2 * pad) / Math.max(maxX - minX, 1e-9), (height - 2 * pad) / Math.max(maxY - minY, 1e-9));
  const offX = (width - (maxX - minX) * scale) / 2;
  const offY = (height - (maxY - minY) * scale) / 2;
  const pt = (i: number): [number, number] => [offX + ((xs[i] as number) - minX) * scale, offY + ((ys[i] as number) - minY) * scale];
  const d = track.map((_, i) => `${i === 0 ? 'M' : 'L'}${pt(i)[0].toFixed(1)} ${pt(i)[1].toFixed(1)}`).join('');
  return { d, start: pt(0), end: pt(track.length - 1), at: pt };
}

/** The path of points `from` to `to` (inclusive) of a drawn route. */
export function routeSlice(at: (i: number) => [number, number], from: number, to: number): string {
  let d = '';
  for (let i = from; i <= to; i += 1) d += `${i === from ? 'M' : 'L'}${at(i)[0].toFixed(1)} ${at(i)[1].toFixed(1)}`;
  return d;
}

function Placeholder({ track, height, label, theme, colors }: { track?: [number, number][]; height: number; label: string; theme: Theme; colors?: readonly string[] | null }) {
  const palette = ROUTE[theme];
  const width = 350;
  const h = Math.round((height / 220) * 150);
  const route = routePath(track, width, h);
  const coloured = !!route && !!colors && colors.length === track?.length;
  const casing = casingFor(theme, coloured);
  return (
    <svg viewBox={`0 0 ${width} ${h}`} role="img" aria-label={label}>
      <rect width={width} height={h} fill={palette.background} />
      {route ? (
        <>
          {casing.width > 0 ? <path d={route.d} fill="none" stroke={casing.color} strokeOpacity={casing.opacity} strokeWidth={casing.width * 0.75} strokeLinejoin="round" strokeLinecap="round" /> : null}
          {coloured && colors ? (
            colorRuns(colors).map((r) => <path key={`${r.from}-${r.color}`} d={routeSlice(route.at, r.from, r.to)} fill="none" stroke={r.color} strokeWidth="3" strokeLinejoin="round" strokeLinecap="round" />)
          ) : (
            <path d={route.d} fill="none" stroke={palette.line} strokeWidth="3" strokeLinejoin="round" strokeLinecap="round" />
          )}
          <circle cx={route.end[0]} cy={route.end[1]} r="4.5" fill={palette.finishFill} stroke={palette.finishRing} strokeWidth="2" />
          <circle cx={route.start[0]} cy={route.start[1]} r="5" fill={palette.start} {...(theme === 'light' ? { stroke: '#fff', strokeWidth: 1.5 } : {})} />
        </>
      ) : null}
    </svg>
  );
}

/** Route map on the owner's PMTiles, or the route drawn alone while tiles are absent. */
export function MapView({ track, tilesUrl = DEFAULT_TILES_URL, label = 'Route map', height = 220, routeColors }: MapViewProps) {
  const container = useRef<HTMLDivElement>(null);
  // The colours change when the viewer picks another value; the map keeps its view and only redraws the line.
  const mapRef = useRef<MapLibreMap | null>(null);
  const colorsRef = useRef(routeColors);
  useEffect(() => {
    colorsRef.current = routeColors;
  }, [routeColors]);
  const theme = useTheme();
  const choice = useMapChoice();
  const [checked, setMode] = useState<'checking' | 'placeholder' | 'map'>('checking');
  // "Route only" (Profile, per browser) never makes the WebGL map.
  const mode = choice === 'route' ? 'placeholder' : checked;

  useEffect(() => {
    let cancelled = false;
    if (choice === 'route') return;
    void tilesAvailable(tilesUrl).then((ok) => {
      if (!cancelled) setMode(ok ? 'map' : 'placeholder');
    });
    return () => {
      cancelled = true;
    };
  }, [tilesUrl, choice]);

  useEffect(() => {
    if (mode !== 'map' || !container.current) return;
    let disposed = false;
    let remove: (() => void) | undefined;

    void (async () => {
      const [maplibre, { Protocol }, worker] = await Promise.all([
        import('maplibre-gl'),
        import('pmtiles'),
        // MapLibre finds its worker next to its own module, which the bundle renames: give Vite's URL.
        import('maplibre-gl/dist/maplibre-gl-worker.mjs?url'),
        import('maplibre-gl/dist/maplibre-gl.css'),
      ]);
      if (disposed || !container.current) return;
      maplibre.setWorkerUrl(worker.default);
      const protocol = new Protocol();
      maplibre.addProtocol('pmtiles', protocol.tile);
      const first = track?.[0];
      let map: InstanceType<typeof maplibre.Map>;
      try {
        map = new maplibre.Map({
          container: container.current,
          style: buildStyle(tilesUrl, theme),
          center: first ?? [10, 56],
          zoom: first ? 12 : 5,
          attributionControl: { compact: true },
          // Go easy on the GPU: no canvas beyond 2x, and no map at all on a slow or blocklisted GPU
          // (the route is drawn alone instead).
          pixelRatio: Math.min(window.devicePixelRatio || 1, 2),
          canvasContextAttributes: { failIfMajorPerformanceCaveat: true },
        });
      } catch {
        maplibre.removeProtocol('pmtiles');
        setMode('placeholder');
        return;
      }
      mapRef.current = map;
      map.addControl(new maplibre.NavigationControl({ showCompass: false }), 'top-right');
      let ready = false;
      const fit = () => {
        if (!ready || !track || track.length < 2) return;
        const lons = track.map((p) => p[0]);
        const lats = track.map((p) => p[1]);
        map.fitBounds(
          [
            [Math.min(...lons), Math.min(...lats)],
            [Math.max(...lons), Math.max(...lats)],
          ],
          // Extra room at the bottom right for the attribution and at the right for the zoom buttons.
          { padding: { top: 32, left: 32, right: 56, bottom: 48 }, animate: false },
        );
      };
      map.on('load', () => {
        ready = true;
        if (!track || track.length < 2) return;
        const initial = casingFor(theme, !!colorsRef.current);
        map.addSource('track', { type: 'geojson', data: routeCollection(track, colorsRef.current, ROUTE[theme].line) });
        // Under the route: a white edge on the light map (clear of white roads), a dark one under colours.
        map.addLayer({
          id: 'track-edge',
          type: 'line',
          source: 'track',
          layout: { 'line-cap': 'round', 'line-join': 'round' },
          paint: { 'line-color': initial.color, 'line-width': initial.width, 'line-opacity': initial.opacity },
        });
        map.addLayer({ id: 'track-line', type: 'line', source: 'track', layout: { 'line-cap': 'round', 'line-join': 'round' }, paint: { 'line-color': ['get', 'c'], 'line-width': theme === 'light' ? 4 : 3 } });
        const colors = ROUTE[theme];
        map.addSource('finish', { type: 'geojson', data: { type: 'Feature', properties: {}, geometry: { type: 'Point', coordinates: track[track.length - 1] as [number, number] } } });
        map.addLayer({ id: 'finish-ring', type: 'circle', source: 'finish', paint: { 'circle-color': colors.finishFill, 'circle-radius': 4.5, 'circle-stroke-color': colors.finishRing, 'circle-stroke-width': 2 } });
        map.addSource('start', { type: 'geojson', data: { type: 'Feature', properties: {}, geometry: { type: 'Point', coordinates: track[0] as [number, number] } } });
        map.addLayer({ id: 'start-dot', type: 'circle', source: 'start', paint: { 'circle-color': colors.start, 'circle-radius': 5, ...(theme === 'light' ? { 'circle-stroke-color': '#ffffff', 'circle-stroke-width': 2 } : {}) } });
        fit();
      });
      // MapLibre follows window resizes only; the page often settles after the map is made (fonts,
      // side column), which left the route fitted to a stale size and running off the edge.
      // Only on a real change of size, at most once a frame, so the canvas is never resized in a loop.
      let size = '';
      let frame = 0;
      const observer =
        typeof ResizeObserver === 'function'
          ? new ResizeObserver((entries) => {
              const box = entries[0]?.contentRect;
              const next = box ? `${Math.round(box.width)}x${Math.round(box.height)}` : '';
              if (next === size) return;
              size = next;
              cancelAnimationFrame(frame);
              frame = requestAnimationFrame(() => {
                map.resize();
                fit();
              });
            })
          : undefined;
      observer?.observe(container.current);
      remove = () => {
        mapRef.current = null;
        observer?.disconnect();
        cancelAnimationFrame(frame);
        map.remove();
        maplibre.removeProtocol('pmtiles');
      };
      if (disposed) remove();
    })();

    return () => {
      disposed = true;
      remove?.();
    };
  }, [mode, tilesUrl, track, theme]);

  // A new choice of colouring: swap the line's data and casing in place.
  useEffect(() => {
    const map = mapRef.current;
    if (mode !== 'map' || !map || !track || track.length < 2) return;
    const source = map.getSource('track') as GeoJSONSource | undefined;
    if (!source) return;
    source.setData(routeCollection(track, routeColors, ROUTE[theme].line));
    const casing = casingFor(theme, !!routeColors);
    map.setPaintProperty('track-edge', 'line-color', casing.color);
    map.setPaintProperty('track-edge', 'line-width', casing.width);
    map.setPaintProperty('track-edge', 'line-opacity', casing.opacity);
  }, [routeColors, mode, track, theme]);

  return (
    <div className="map">
      {mode === 'map' ? (
        <div ref={container} className="map__canvas" style={{ height }} role="group" aria-label={label} />
      ) : (
        <>
          <Placeholder track={track} height={height} label={label} theme={theme} colors={routeColors} />
          {mode === 'checking' ? <p className="map__caption">Loading map.</p> : null}
        </>
      )}
    </div>
  );
}
