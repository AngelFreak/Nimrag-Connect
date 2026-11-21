import { useEffect, useRef, useState } from 'react';
import type { MapOptions } from 'maplibre-gl';

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

/** Dark base style for the Protomaps vector schema, without labels (no glyph server needed). */
export function buildStyle(pmtilesUrl: string): MapStyle {
  const absolute = new URL(pmtilesUrl, window.location.origin).href;
  return {
    version: 8,
    sources: {
      base: { type: 'vector', url: `pmtiles://${absolute}`, attribution: '© OpenStreetMap contributors' },
    },
    layers: [
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
        paint: { 'line-color': '#2b373d', 'line-width': ['interpolate', ['linear'], ['zoom'], 8, 0.5, 14, 2, 18, 8] },
      },
    ],
  };
}

interface MapViewProps {
  /** Route as [longitude, latitude] pairs. */
  track?: [number, number][];
  tilesUrl?: string;
  /** Accessible name and summary of the route. The map is a group (not an image) because it holds MapLibre's zoom buttons; the stats beside it are what a screen reader reads. */
  label?: string;
  height?: number;
}

/**
 * The route drawn on its own, without a base map, for when the server has no tile file. Longitude
 * is scaled by cos(latitude) so the shape is not stretched; with no track there is no drawing.
 */
export function routePath(track: [number, number][] | undefined, width: number, height: number, pad = 12): { d: string; start: [number, number] } | null {
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
  return { d, start: pt(0) };
}

function Placeholder({ track, height, label }: { track?: [number, number][]; height: number; label: string }) {
  const width = 350;
  const h = Math.round((height / 220) * 150);
  const route = routePath(track, width, h);
  return (
    <svg viewBox={`0 0 ${width} ${h}`} role="img" aria-label={label}>
      <rect width={width} height={h} fill="#141b1f" />
      {route ? (
        <>
          <path d={route.d} fill="none" stroke="#7dbbff" strokeWidth="3" strokeLinejoin="round" strokeLinecap="round" />
          <circle cx={route.start[0]} cy={route.start[1]} r="5" fill="#ffb066" />
        </>
      ) : null}
    </svg>
  );
}

/** Route map on the owner's PMTiles, or the route drawn alone while tiles are absent. */
export function MapView({ track, tilesUrl = DEFAULT_TILES_URL, label = 'Route map', height = 220 }: MapViewProps) {
  const container = useRef<HTMLDivElement>(null);
  const [mode, setMode] = useState<'checking' | 'placeholder' | 'map'>('checking');

  useEffect(() => {
    let cancelled = false;
    void tilesAvailable(tilesUrl).then((ok) => {
      if (!cancelled) setMode(ok ? 'map' : 'placeholder');
    });
    return () => {
      cancelled = true;
    };
  }, [tilesUrl]);

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
      const map = new maplibre.Map({
        container: container.current,
        style: buildStyle(tilesUrl),
        center: first ?? [10, 56],
        zoom: first ? 12 : 5,
        attributionControl: { compact: true },
      });
      map.addControl(new maplibre.NavigationControl({ showCompass: false }), 'top-right');
      map.on('load', () => {
        if (!track || track.length < 2) return;
        map.addSource('track', { type: 'geojson', data: { type: 'Feature', properties: {}, geometry: { type: 'LineString', coordinates: track } } });
        map.addLayer({ id: 'track-line', type: 'line', source: 'track', layout: { 'line-cap': 'round', 'line-join': 'round' }, paint: { 'line-color': '#7dbbff', 'line-width': 3 } });
        map.addSource('start', { type: 'geojson', data: { type: 'Feature', properties: {}, geometry: { type: 'Point', coordinates: track[0] as [number, number] } } });
        map.addLayer({ id: 'start-dot', type: 'circle', source: 'start', paint: { 'circle-color': '#ffb066', 'circle-radius': 5 } });
        const lons = track.map((p) => p[0]);
        const lats = track.map((p) => p[1]);
        map.fitBounds(
          [
            [Math.min(...lons), Math.min(...lats)],
            [Math.max(...lons), Math.max(...lats)],
          ],
          { padding: 32, animate: false },
        );
      });
      remove = () => {
        map.remove();
        maplibre.removeProtocol('pmtiles');
      };
      if (disposed) remove();
    })();

    return () => {
      disposed = true;
      remove?.();
    };
  }, [mode, tilesUrl, track]);

  return (
    <div className="map">
      {mode === 'map' ? (
        <div ref={container} className="map__canvas" style={{ height }} role="group" aria-label={label} />
      ) : (
        <>
          <Placeholder track={track} height={height} label={label} />
          {mode === 'checking' ? <p className="map__caption">Loading map.</p> : null}
        </>
      )}
    </div>
  );
}
