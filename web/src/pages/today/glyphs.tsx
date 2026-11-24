import type { ReactNode } from 'react';

// Small line icons for the Home cards and rows (24 px grid, 2 px stroke, round caps), in the same style
// as components/icons.tsx. They are local to Home so other pages' icon sets are not touched.

function G({ size = 20, children }: { size?: number; children: ReactNode }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
      {children}
    </svg>
  );
}

export type GlyphName =
  | 'energy'
  | 'activities'
  | 'run'
  | 'cycle'
  | 'walk'
  | 'hike'
  | 'swim'
  | 'strength'
  | 'training'
  | 'other'
  | 'steps'
  | 'sleep'
  | 'status'
  | 'heart'
  | 'stress'
  | 'hrv'
  | 'intensity'
  | 'load'
  | 'recovery'
  | 'vo2'
  | 'breathing'
  | 'spo2'
  | 'weight'
  | 'readiness'
  | 'workout'
  | 'watch';

const PATHS: Record<GlyphName, ReactNode> = {
  // a lightning bolt
  energy: <path d="M13 3L5 13.5h6L10 21l8-10.5h-6z" />,
  // a person in motion
  activities: (
    <>
      <circle cx="13" cy="4.5" r="1.8" />
      <path d="M8 21l3-6.5-2.5-3 4-3 3 3.5 3 .5M9 9.5L6 11" />
    </>
  ),
  run: (
    <>
      <circle cx="14.5" cy="4.5" r="1.8" />
      <path d="M5 21l3.5-5 3 2.5 2-5.5-3-2.5 2.5-2.5 3 3 3.5 1M8 11l3-1.5" />
    </>
  ),
  cycle: (
    <>
      <circle cx="6" cy="16" r="3.5" />
      <circle cx="18" cy="16" r="3.5" />
      <path d="M6 16l4-7h5l3 7M10 9l3 7M9 6h3" />
    </>
  ),
  walk: (
    <>
      <circle cx="12.5" cy="4.5" r="1.8" />
      <path d="M9 21l2.5-7-2-3 3.5-3 2.5 3.5 3 1M11.5 14l3 2 .5 5M9 9.5L7 12" />
    </>
  ),
  hike: (
    <>
      <path d="M3 20l6.5-11 3.5 6 2-3L21 20z" />
      <path d="M9.5 9l1.5 2.5" />
    </>
  ),
  swim: (
    <>
      <circle cx="16" cy="6" r="1.8" />
      <path d="M4 12l5-3 4 2.5M2.5 17c1.5 0 1.5-1 3-1s1.5 1 3 1 1.5-1 3-1 1.5 1 3 1 1.5-1 3-1 1.5 1 3 1" />
    </>
  ),
  strength: <path d="M6 8v8M18 8v8M3 10v4M21 10v4M6 12h12" />,
  training: (
    <>
      <circle cx="12" cy="13.5" r="7" />
      <path d="M12 10v4l2.5 1.5M9.5 3h5" />
    </>
  ),
  other: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M8 12h.01M12 12h.01M16 12h.01" />
    </>
  ),
  // two footprints
  steps: (
    <>
      <ellipse cx="8" cy="8" rx="2.5" ry="3.5" />
      <path d="M6 14h4v1.5a2 2 0 0 1-4 0z" />
      <ellipse cx="16" cy="12.5" rx="2.5" ry="3.5" />
      <path d="M14 18.5h4V20a2 2 0 0 1-4 0z" />
    </>
  ),
  // a moon
  sleep: <path d="M19.5 14.5A8 8 0 0 1 9.5 4.5a8 8 0 1 0 10 10z" />,
  // a rising trend
  status: (
    <>
      <path d="M3 17l5-5 4 3 7-8" />
      <path d="M15 7h4v4" />
    </>
  ),
  heart: <path d="M12 20s-7.5-4.5-7.5-10A4.2 4.2 0 0 1 12 7.5 4.2 4.2 0 0 1 19.5 10c0 5.5-7.5 10-7.5 10z" />,
  stress: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 12l4-3M7.5 15.5h.01M12 6.5h.01M16.5 15.5h.01" />
    </>
  ),
  hrv: (
    <>
      <path d="M12 20s-7.5-4.5-7.5-10A4.2 4.2 0 0 1 12 7.5 4.2 4.2 0 0 1 19.5 10c0 5.5-7.5 10-7.5 10z" />
      <path d="M7 12.5h2.5l1.5-2.5 2 4 1.5-1.5H17" />
    </>
  ),
  intensity: (
    <>
      <circle cx="13.5" cy="13.5" r="6.5" />
      <path d="M13.5 10v3.5l2 1.5M11.5 3h4M2.5 11h3M2.5 15h3" />
    </>
  ),
  load: <path d="M4 19V10M9 19V5M14 19v-7M19 19V8" />,
  recovery: (
    <>
      <path d="M20 12a8 8 0 1 1-2.5-5.8" />
      <path d="M20 4v4.5h-4.5M12 8v4l2.5 1.5" />
    </>
  ),
  vo2: (
    <>
      <path d="M12 4v8" />
      <path d="M12 12c-1.5 0-2.5 1-3.5 2.5S5 18.5 5 18.5s-1-4 .5-7C7 8.5 9.5 7 12 7" />
      <path d="M12 12c1.5 0 2.5 1 3.5 2.5s3.5 4 3.5 4 1-4-.5-7C17 8.5 14.5 7 12 7" />
    </>
  ),
  breathing: <path d="M3 12c2 0 2-5 4-5s2 10 4 10 2-10 4-10 2 5 4 5h2" />,
  spo2: <path d="M12 3.5s6 6 6 10.5a6 6 0 0 1-12 0c0-4.5 6-10.5 6-10.5z" />,
  weight: (
    <>
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <path d="M8.5 9a4.5 4.5 0 0 1 7 0M12 9l1.5-1.5" />
    </>
  ),
  readiness: (
    <>
      <path d="M4 16a8 8 0 1 1 16 0" />
      <path d="M12 16l3.5-5" />
    </>
  ),
  workout: (
    <>
      <path d="M5 4h14v16H5z" />
      <path d="M9 9h6M9 13h6M9 17h3" />
    </>
  ),
  watch: (
    <>
      <rect x="7" y="6" width="10" height="12" rx="3" />
      <path d="M9 6l.5-3h5l.5 3M9 18l.5 3h5l.5-3M12 10v2.5l1.5 1" />
    </>
  ),
};

/** The sport's glyph for an activity `kind` (run, cycle, walk, hike, swim, strength, training, other). */
export function glyphForKind(kind: string): GlyphName {
  switch (kind) {
    case 'run':
      return 'run';
    case 'cycle':
      return 'cycle';
    case 'walk':
      return 'walk';
    case 'hike':
      return 'hike';
    case 'swim':
      return 'swim';
    case 'strength':
      return 'strength';
    case 'training':
      return 'training';
    default:
      return 'other';
  }
}

export function Glyph({ name, size = 20 }: { name: GlyphName; size?: number }) {
  return <G size={size}>{PATHS[name]}</G>;
}
