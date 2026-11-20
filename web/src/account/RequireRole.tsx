import { useEffect } from 'react';
import { Navigate, Outlet, useLocation } from 'react-router-dom';
import { AppShell, Skeleton } from '../components';
import { hasNativeSession, notifySessionExpired } from '../native';
import { NotFoundPage } from '../pages/NotFoundPage';
import type { Role } from '../api/generated/Role';
import { NativeSessionWait } from './NativeSessionWait';
import { useMe } from './queries';

/**
 * Route guard driven by GET /api/me.
 * - nobody signed in: the login page, remembering where the person was going (inside the
 *   Android app the app signs in natively and reloads instead; no login page there);
 * - role `user` on an admin route: the "page not found" screen (the admin area is not advertised);
 * - role `admin` on a data route: the admin area, which is all an admin sees (the server answers
 *   every data endpoint with 403 for an admin session, so nothing would load there anyway).
 * When /api/me cannot be reached (offline), the page renders and shows its own error or cached data.
 */
export function RequireRole({ allow }: { allow: Role }) {
  const me = useMe();
  const location = useLocation();
  const nativeTakesOver = me.data === null && hasNativeSession();
  useEffect(() => {
    if (nativeTakesOver) notifySessionExpired();
  }, [nativeTakesOver]);

  if (me.isPending) {
    return (
      <main className="center-page" aria-busy="true">
        <Skeleton height={20} width="160px" />
      </main>
    );
  }
  if (nativeTakesOver) return <NativeSessionWait />;
  if (me.data === null) {
    const here = location.pathname + location.search;
    return <Navigate to={`/login?next=${encodeURIComponent(here)}`} replace />;
  }
  if (me.data && me.data.role !== allow) {
    if (allow === 'admin') {
      return (
        <AppShell>
          <NotFoundPage />
        </AppShell>
      );
    }
    return <Navigate to="/admin" replace />;
  }
  return <Outlet />;
}
