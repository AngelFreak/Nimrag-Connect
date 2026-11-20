import { Link, Outlet, useLocation } from 'react-router-dom';
import { useMe } from '../../account/queries';
import { sectionFromSearch } from './ProfilePage';

type NavKey = 'account' | 'preferences' | 'phones' | 'garmin' | 'about';

const NAV: { key: NavKey; label: string; to: string }[] = [
  { key: 'account', label: 'Account', to: '/profile' },
  { key: 'preferences', label: 'Preferences', to: '/profile?section=preferences' },
  { key: 'phones', label: 'Phones', to: '/profile/phones' },
  { key: 'garmin', label: 'Garmin Connect', to: '/profile/garmin' },
  { key: 'about', label: 'About', to: '/profile?section=about' },
];

/** Which section of the list a page belongs to: the pages below Account and Preferences belong to them. */
export function navKeyForProfile(pathname: string, search: string): NavKey {
  if (pathname.startsWith('/profile/phones')) return 'phones';
  if (pathname.startsWith('/profile/garmin')) return 'garmin';
  if (pathname.startsWith('/profile/timezone') || pathname.startsWith('/profile/goals')) return 'preferences';
  if (pathname === '/profile' || pathname === '/profile/') return sectionFromSearch(search);
  return 'account';
}

/**
 * Frame of every profile page. A phone shows the page alone. From 1024 px a section list stands on the
 * left (Account, Preferences, Phones, Garmin Connect, About) and the page, whether the list or one of
 * its small pages, fills the right-hand pane. An admin has only one section, so no list.
 */
export function ProfileLayout() {
  const me = useMe();
  const { pathname, search } = useLocation();
  const admin = me.data?.role === 'admin';
  const current = navKeyForProfile(pathname, search);
  return (
    <div className={admin ? 'profile-layout profile-layout--solo' : 'profile-layout'}>
      {admin ? null : (
        <nav className="profile-nav" aria-label="Profile sections">
          <ul>
            {NAV.map((n) => (
              <li key={n.key}>
                <Link to={n.to} aria-current={current === n.key ? 'page' : undefined}>
                  {n.label}
                </Link>
              </li>
            ))}
          </ul>
        </nav>
      )}
      <div className="profile-content">
        <Outlet />
      </div>
    </div>
  );
}
