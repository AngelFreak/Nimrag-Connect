import { Skeleton } from '../components';

/**
 * Shown instead of the login page inside the Android app while the app signs in again
 * (fingerprint or password) and reloads the page. The app's own screen covers this one.
 */
export function NativeSessionWait() {
  return (
    <main className="center-page" aria-busy="true" aria-label="Signing in with the app">
      <Skeleton height={20} width="160px" />
    </main>
  );
}
