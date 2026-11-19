import type { ReactNode } from 'react';

// Inline stroke icons in the canvas's style: 24 px grid, 2 px stroke, round caps and joins.
// Decorative: the button or link that holds an icon carries the accessible name.

interface IconProps {
  size?: number;
}

function Svg({ size = 22, children }: IconProps & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

export const BackIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M15 5l-7 7 7 7" />
  </Svg>
);
export const NextIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M9 5l7 7-7 7" />
  </Svg>
);
export const TodayIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 3v2M12 19v2M3 12h2M19 12h2" />
  </Svg>
);
export const ActivitiesIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 17l5-6 4 3 4-7 5 6" />
  </Svg>
);
export const HealthIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 12h4l2-5 4 10 2-5h6" />
  </Svg>
);
export const StrengthIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 8v8M18 8v8M3 10v4M21 10v4M6 12h12" />
  </Svg>
);
export const MoreIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M5 12h.01M12 12h.01M19 12h.01" />
  </Svg>
);
export const UserIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="8" r="4" />
    <path d="M4 21c0-4 3.6-7 8-7s8 3 8 7" />
  </Svg>
);
export const RefreshIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20 11a8 8 0 0 0-14.5-4M4 4v4h4" />
    <path d="M4 13a8 8 0 0 0 14.5 4M20 20v-4h-4" />
  </Svg>
);
export const WatchIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="7" y="7" width="10" height="10" rx="3" />
    <path d="M9 7l.7-4h4.6l.7 4M9 17l.7 4h4.6l.7-4" />
    <path d="M12 10v2l1.5 1" />
  </Svg>
);
export const AlertIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 8v5M12 16h.01" />
  </Svg>
);
export const InboxIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 13l3-8h12l3 8v6H3zM3 13h5l1 3h6l1-3h5" />
  </Svg>
);
