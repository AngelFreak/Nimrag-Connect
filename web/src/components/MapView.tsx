import { useEffect, useRef, useState } from 'react';
import type { MapOptions } from 'maplibre-gl';

// Maps use MapLibre GL JS with PMTiles from the owner's own server (/tiles/*.pmtiles).
// There is no third-party tile server and no hosted style or glyph URL: when the tile file
// is missing the design's placeholder is shown instead. MapLibre itself is loaded lazily,
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
  /** Accessible name; the map is not keyboard-operable content, the stats beside it are. */
  label?: string;
  height?: number;
}

function Placeholder() {
  return (
    <svg viewBox="0 0 350 150" preserveAspectRatio="none" role="img" aria-label="Route map">
      <rect width="350" height="150" fill="#141b1f" />
      <path d="M0 40H350M0 80H350M0 120H350M60 0V150M130 0V150M200 0V150M270 0V150" stroke="#1f292e" strokeWidth="1" />
      <path d="M40 110C70 60 110 40 150 52S220 30 262 44S318 92 286 112S190 128 150 118S70 132 40 110Z" fill="none" stroke="#7dbbff" strokeWidth="3" strokeLinejoin="round" />
      <circle cx="40" cy="110" r="5" fill="#ffb066" />
    </svg>
  );
}

/** Route map on the owner's PMTiles, or the design's placeholder while tiles are absent. */
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
      const [maplibre, { Protocol }] = await Promise.all([import('maplibre-gl'), import('pmtiles'), import('maplibre-gl/dist/maplibre-gl.css')]);
      if (disposed || !container.current) return;
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
        <div ref={container} className="map__canvas" style={{ height }} role="img" aria-label={label} />
      ) : (
        <>
          <Placeholder />
          {mode === 'checking' ? <p className="map__caption">Loading map.</p> : null}
        </>
      )}
    </div>
  );
}
