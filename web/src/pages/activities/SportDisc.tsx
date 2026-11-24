import type { ReactNode } from 'react';
import { ActivitiesIcon, BoltIcon, CycleIcon, HeartIcon, HikeIcon, RunIcon, StrengthIcon, SwimIcon, WalkIcon } from '../../components';

type Glyph = (size: number) => ReactNode;

/** The glyph of each sport kind the API knows; anything else gets the "other" heart. */
const GLYPHS: Record<string, Glyph> = {
  all: (size) => <ActivitiesIcon size={size} />,
  run: (size) => <RunIcon size={size} />,
  cycle: (size) => <CycleIcon size={size} />,
  walk: (size) => <WalkIcon size={size} />,
  hike: (size) => <HikeIcon size={size} />,
  swim: (size) => <SwimIcon size={size} />,
  strength: (size) => <StrengthIcon size={size} />,
  training: (size) => <BoltIcon size={size} />,
  other: (size) => <HeartIcon size={size} />,
};

/** The glyph of a sport kind at a size, as an element. */
export function sportIcon(kind: string, size: number): ReactNode {
  return (GLYPHS[kind] ?? GLYPHS['other']!)(size);
}

/**
 * Round sport icon. `tone="solid"` is the white glyph on the sport's colour (list rows); `"soft"` is the
 * grey disc of the activity page title. Decorative: the name next to it says the sport.
 */
export function SportDisc({ kind, size = 40, tone = 'solid' }: { kind: string; size?: number; tone?: 'solid' | 'soft' }) {
  return (
    <span className={`sport-disc sport-disc--${tone}`} data-kind={GLYPHS[kind] ? kind : 'other'} style={{ width: size, height: size }} aria-hidden="true">
      {sportIcon(kind, Math.round(size * 0.55))}
    </span>
  );
}
