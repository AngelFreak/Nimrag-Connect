import { useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Navigate, Route, Routes, useNavigate } from 'react-router-dom';
import { ME_KEY, useMe } from './account/queries';
import { RequireRole } from './account/RequireRole';
import { setUnauthorizedHandler } from './api/client';
import { AppShell } from './components';
import { useNativeRouting } from './lib/hooks';
import { ActivitiesPage } from './pages/ActivitiesPage';
import { ActivityDetailPage } from './pages/ActivityDetailPage';
import { AdminPage } from './pages/AdminPage';
import { BodyPage } from './pages/BodyPage';
import { CalendarPage } from './pages/CalendarPage';
import { DayPage } from './pages/DayPage';
import { GalleryPage } from './pages/GalleryPage';
import { HealthPage } from './pages/HealthPage';
import { HrvPage } from './pages/HrvPage';
import { LoginPage } from './pages/LoginPage';
import { MetricPage } from './pages/MetricPage';
import { MovementPage } from './pages/MovementPage';
import { NotFoundPage } from './pages/NotFoundPage';
import { SetupPage } from './pages/SetupPage';
import { SleepPage } from './pages/SleepPage';
import { StrengthPage } from './pages/StrengthPage';
import { TodayPage } from './pages/TodayPage';
import { TrainingPage } from './pages/TrainingPage';
import { TrainingStatusPage } from './pages/TrainingStatusPage';
import { WatchPage } from './pages/WatchPage';
import { WorkoutPage } from './pages/WorkoutPage';
import { WorkoutListPage } from './pages/workouts/WorkoutListPage';
import { GarminPage } from './pages/profile/GarminPage';
import { GoalsPage } from './pages/profile/GoalsPage';
import { PasswordPage } from './pages/profile/PasswordPage';
import { PhonesPage } from './pages/profile/PhonesPage';
import { ProfileLayout } from './pages/profile/ProfileLayout';
import { ProfilePage } from './pages/profile/ProfilePage';
import { TimezonePage } from './pages/profile/TimezonePage';
import { TwoStepPage } from './pages/profile/TwoStepPage';
import { WatchDetailsPage } from './pages/watch/WatchDetailsPage';

/** The profile belongs to everyone who is signed in: an admin gets the admin frame, a user the data frame. */
function ProfileShell() {
  const me = useMe();
  return <AppShell area={me.data?.role === 'admin' ? 'admin' : 'data'} />;
}

export function App() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  // Android shell: report every page to the app and follow its tab taps (no-op in a browser).
  useNativeRouting();

  // A 401 from the API sends the user to the login page without a full reload.
  useEffect(() => {
    setUnauthorizedHandler(() => {
      const here = window.location.pathname + window.location.search;
      if (window.location.pathname.startsWith('/login') || window.location.pathname.startsWith('/setup')) return;
      // The session is gone: forget who was signed in, so the route guard asks again.
      queryClient.setQueryData(ME_KEY, null);
      void navigate(`/login?next=${encodeURIComponent(here)}`, {
        replace: true,
      });
    });
    return () => setUnauthorizedHandler(null);
  }, [navigate, queryClient]);

  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route path="/setup" element={<SetupPage />} />
      {/* Component gallery: no data, no session needed. */}
      <Route element={<AppShell tabs />}>
        <Route path="__gallery" element={<GalleryPage />} />
      </Route>
      {/* Settings moved to the profile; old links and bookmarks still work. */}
      <Route path="settings" element={<Navigate to="/profile" replace />} />
      {/* The profile: password and two-step login for every account; the rest only for people with data. */}
      <Route element={<RequireRole allow="any" />}>
        <Route element={<ProfileShell />}>
          <Route path="profile" element={<ProfileLayout />}>
            <Route index element={<ProfilePage />} />
            <Route path="password" element={<PasswordPage />} />
            <Route path="two-step" element={<TwoStepPage />} />
            <Route element={<RequireRole allow="user" />}>
              <Route path="phones" element={<PhonesPage />} />
              <Route path="timezone" element={<TimezonePage />} />
              <Route path="goals" element={<GoalsPage />} />
              <Route path="garmin" element={<GarminPage />} />
            </Route>
          </Route>
        </Route>
      </Route>
      {/* The admin area: the only thing an admin sees. A normal user gets "page not found". */}
      <Route element={<RequireRole allow="admin" />}>
        <Route element={<AppShell area="admin" />}>
          <Route path="admin" element={<AdminPage />} />
        </Route>
      </Route>
      {/* Everything with data belongs to users; an admin session is sent to /admin. */}
      <Route element={<RequireRole allow="user" />}>
        {/* Main tabs: top bar (profile, sync, watch) and bottom navigation on a phone. */}
        <Route element={<AppShell tabs />}>
          <Route index element={<TodayPage />} />
          <Route path="activities" element={<ActivitiesPage />} />
          <Route path="strength" element={<ActivitiesPage strength />} />
          <Route path="health" element={<HealthPage />} />
        </Route>
        {/* Detail pages: their own back button, no bottom navigation. */}
        <Route element={<AppShell />}>
          <Route path="activities/:id" element={<ActivityDetailPage />} />
          <Route path="activities/:id/strength" element={<StrengthPage />} />
          <Route path="sleep/:date?" element={<SleepPage />} />
          <Route path="day/:date?" element={<DayPage />} />
          <Route path="movement/:date?" element={<MovementPage />} />
          <Route path="metrics/:metric" element={<MetricPage />} />
          <Route path="training" element={<TrainingPage />} />
          <Route path="training/status" element={<TrainingStatusPage />} />
          <Route path="hrv" element={<HrvPage />} />
          <Route path="body" element={<BodyPage />} />
          <Route path="calendar/:month?" element={<CalendarPage />} />
          <Route path="workouts" element={<WorkoutListPage />} />
          <Route path="workouts/new" element={<WorkoutPage />} />
          <Route path="workouts/:id" element={<WorkoutPage />} />
          <Route path="devices" element={<WatchPage />} />
          <Route path="devices/details" element={<WatchDetailsPage />} />
          <Route path="*" element={<NotFoundPage />} />
        </Route>
      </Route>
    </Routes>
  );
}
