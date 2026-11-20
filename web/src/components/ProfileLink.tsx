import { Link, useLocation } from 'react-router-dom';
import { UserIcon } from './icons';

/** The avatar button of the top bar: a plain link to the profile page. */
export function ProfileLink() {
  const { pathname } = useLocation();
  const here = pathname === '/profile' || pathname.startsWith('/profile/');
  return (
    <Link to="/profile" className="icon-btn" aria-label="Profile" aria-current={here ? 'page' : undefined}>
      <UserIcon />
    </Link>
  );
}
