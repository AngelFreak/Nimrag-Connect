import { useIsDesktop, usePageTitle } from '../lib/hooks';
import { DesktopOverview } from './today/DesktopOverview';
import { PhoneToday } from './today/PhoneToday';
import { useSettings, useToday } from './today/queries';
import './today/today.css';

/**
 * Today (stage 3c). On a phone: In focus, At a glance, Yesterday, Last 7 days, Edit home.
 * From 1024 px: Home in the web layout (In Focus, At a Glance, Yesterday, Last 7 Days; activities, sync and workouts at the right). Both read GET /api/today and the card layout
 * from GET /api/settings (`home_cards`).
 */
export function TodayPage() {
  const desktop = useIsDesktop();
  usePageTitle(desktop ? 'Home' : 'Today');
  const today = useToday();
  const settings = useSettings();
  // The desktop panels fetch their own data, so only the desktop layout asks for it.
  return desktop ? <DesktopOverview today={today} settings={settings} /> : <PhoneToday today={today} settings={settings} />;
}
