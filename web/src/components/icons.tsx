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

// Settings rows (components/SettingsRow.tsx): one simple stroke symbol per kind of setting.
export const KeyIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="8" cy="15" r="4" />
    <path d="M11 12l9-9M16 7l3 3M14 9l2 2" />
  </Svg>
);
export const ShieldIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 3l8 3v6c0 4.5-3.2 7.8-8 9-4.8-1.2-8-4.5-8-9V6z" />
    <path d="M8.5 12l2.5 2.5 4.5-5" />
  </Svg>
);
export const PhoneIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="7" y="2.5" width="10" height="19" rx="2.5" />
    <path d="M11 18.5h2" />
  </Svg>
);
export const GlobeIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M3 12h18M12 3c2.5 2.6 3.8 5.6 3.8 9s-1.3 6.4-3.8 9c-2.5-2.6-3.8-5.6-3.8-9S9.5 5.6 12 3z" />
  </Svg>
);
export const TargetIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <circle cx="12" cy="12" r="5" />
    <circle cx="12" cy="12" r="1" />
  </Svg>
);
export const SendIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 16V4M7 9l5-5 5 5" />
    <path d="M4 15v4a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-4" />
  </Svg>
);
export const DownloadIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 4v12M7 11l5 5 5-5" />
    <path d="M4 15v4a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-4" />
  </Svg>
);
/** An arrow going into a panel: bring a file in. */
export const ImportIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 12h11M10 8l4 4-4 4" />
    <path d="M14 5h5a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1h-5" />
  </Svg>
);
export const InfoIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 11v5M12 8h.01" />
  </Svg>
);
export const LogoutIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M14 4h4a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-4" />
    <path d="M10 8l-4 4 4 4M6 12h10" />
  </Svg>
);
export const ServerIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="4" width="18" height="6.5" rx="2" />
    <rect x="3" y="13.5" width="18" height="6.5" rx="2" />
    <path d="M7 7.25h.01M7 16.75h.01" />
  </Svg>
);
export const FilesIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 3h8l4 4v14H6z" />
    <path d="M14 3v4h4" />
  </Svg>
);
export const DetailsIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 7h16M4 12h16M4 17h10" />
  </Svg>
);
export const UsersIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="9" cy="8" r="3.5" />
    <path d="M2.5 20c0-3.3 2.9-5.5 6.5-5.5s6.5 2.2 6.5 5.5M16 4.8a3.5 3.5 0 0 1 0 6.4M18 14.8c2 .7 3.5 2.4 3.5 5.2" />
  </Svg>
);
export const BatteryIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="7" width="16" height="10" rx="2.5" />
    <path d="M21.5 10.5v3" />
  </Svg>
);

// Desktop shell and page headers (components/AppShell.tsx, components/PageHeader.tsx).
export const HomeIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3.5 11L12 4l8.5 7M6 9.5V20h12V9.5M10 20v-5h4v5" />
  </Svg>
);
export const CalendarIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="4" y="5" width="16" height="15" rx="2" />
    <path d="M4 10h16M8 3v4M16 3v4" />
  </Svg>
);
export const ChevronDownIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 9l6 6 6-6" />
  </Svg>
);
export const PerformanceIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M5 20V12M10 20V6M15 20V10M20 20V4" />
  </Svg>
);
export const PlanIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="13.5" r="7" />
    <path d="M12 10v4l2.5 1.5M9.5 3h5" />
  </Svg>
);
export const SunIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2.5v2.5M12 19v2.5M2.5 12H5M19 12h2.5M5.3 5.3l1.8 1.8M16.9 16.9l1.8 1.8M5.3 18.7l1.8-1.8M16.9 7.1l1.8-1.8" />
  </Svg>
);
export const HelpIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M9.5 9.5a2.5 2.5 0 1 1 3.6 2.3c-.7.4-1.1 1-1.1 1.7M12 17h.01" />
  </Svg>
);
export const KebabIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 5h.01M12 12h.01M12 19h.01" />
  </Svg>
);
export const SearchIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="11" cy="11" r="6.5" />
    <path d="M16 16l4.5 4.5" />
  </Svg>
);

// Sport glyphs for the activity lists and pages (white on a coloured disc, or grey on a light one).
export const RunIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="15.5" cy="4.8" r="1.8" />
    <path d="M13.8 8L11 13M13.2 9.4l3.3 2M12.3 9.7L9 10.8 8 13.2M11 13l3 3.5-1.5 4M11 13l-3 3.5H4.5" />
  </Svg>
);
export const WalkIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="13" cy="4.5" r="1.8" />
    <path d="M13 8.2v5.3l-3 6.5M13 13.5l3 6.5M13 9.4l-3.2 2.4M13 9.4l3.2 3" />
  </Svg>
);
export const HikeIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 19.5L9.5 8l3.7 6.3 2.3-3.3 5.5 8.5H3zM9.5 8l1.6 2.8" />
  </Svg>
);
export const CycleIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="6" cy="16" r="3.6" />
    <circle cx="18" cy="16" r="3.6" />
    <path d="M6 16l3.8-7h5.2l3 7M9.8 9l3.4 7M13 5.5h2.8M9 9h-1.5" />
  </Svg>
);
export const SwimIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="16.5" cy="6" r="1.8" />
    <path d="M4.5 13l5.5-3.3 4 2 2.2-1.4M3 17c1.5 0 1.5-1 3-1s1.5 1 3 1 1.5-1 3-1 1.5 1 3 1 1.5-1 3-1 1.5 1 3 1M3 21c1.5 0 1.5-1 3-1s1.5 1 3 1 1.5-1 3-1 1.5 1 3 1 1.5-1 3-1 1.5 1 3 1" />
  </Svg>
);
export const BoltIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M13 3L5.5 13.2H11L10 21l7.5-10.2H12L13 3z" />
  </Svg>
);
export const HeartIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 20s-7.5-4.6-7.5-10.2A4.2 4.2 0 0 1 12 7.4a4.2 4.2 0 0 1 7.5 2.4C19.5 15.4 12 20 12 20z" />
  </Svg>
);
