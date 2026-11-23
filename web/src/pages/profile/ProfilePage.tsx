import { useEffect, useState } from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { useLogout, useMe } from '../../account/queries';
import {
  Chip,
  ConfirmDialog,
  DownloadIcon,
  FilesIcon,
  GlobeIcon,
  ImportIcon,
  InfoIcon,
  KeyIcon,
  LogoutIcon,
  PhoneIcon,
  SendIcon,
  SettingsGroup,
  SettingsRow,
  ShieldIcon,
  TargetIcon,
  TopBar,
  UsersIcon,
} from '../../components';
import { todayIn } from '../../lib/dates';
import { fmtInt } from '../../lib/format';
import { usePageTitle } from '../../lib/hooks';
import { useSettings } from '../today/queries';
import { useAbout, useGarmin, useTokens } from '../system/queries';
import { breakableZone, phonesSummary } from './common';
import { GARMIN_STATE_VALUE } from './GarminPage';

/** Where the code lives; the About page links here. */
export const SOURCE_URL = 'https://github.com/AngelFreak/Nimrag-Connect';
export const LICENCE = 'AGPL-3.0';
/** The signed-in user's data as a zip (originals and CSV files). A plain link, so the browser and the app's web view both save it. */
export const EXPORT_URL = '/api/export';

/** The server's name for the export zip (server/src/web_api/export.rs), for the app to save it under. */
export function exportFileName(username: string | undefined, day: string): string {
  return username ? `nimrag-export-${username}-${day}.zip` : `nimrag-export-${day}.zip`;
}

export type ProfileSection = 'account' | 'preferences' | 'about';

/** From 1024 px the home list shows one section at a time; `?section=` picks it (a phone shows them all). */
export function sectionFromSearch(search: string): ProfileSection {
  const section = new URLSearchParams(search).get('section');
  return section === 'preferences' || section === 'about' ? section : 'account';
}

function Head({ name, host, admin }: { name: string | undefined; host: string; admin: boolean }) {
  return (
    <section className="profile-head" aria-label="Your account">
      <span className="profile-head__avatar" aria-hidden="true">
        {(name ?? '?').slice(0, 1)}
      </span>
      <div className="profile-head__text">
        <p className="profile-head__name">{name ?? 'Your account'}</p>
        <p className="profile-head__host">{admin ? `${host} · Admin` : host}</p>
        <Chip>Two-step login on</Chip>
      </div>
    </section>
  );
}

/** Rows that need the person's own data: only for role `user`, so an admin never asks for them. */
function UserRows() {
  const me = useMe();
  const settings = useSettings();
  const tokens = useTokens();
  const garmin = useGarmin();
  const about = useAbout();
  const s = settings.data;
  const settingValue = (text: string | null) => (s ? text : settings.isError ? 'Could not load' : undefined);
  return (
    <>
      <div data-pane="account">
        <SettingsGroup title="Account">
          <SettingsRow icon={<KeyIcon />} tone="accent" title="Password" to="/profile/password" />
          <SettingsRow icon={<ShieldIcon />} tone="energy" title="Two-step login" value="On" to="/profile/two-step" />
          <SettingsRow
            icon={<PhoneIcon />}
            tone="sleep"
            title="Signed-in phones"
            value={tokens.data ? phonesSummary(tokens.data.tokens) : tokens.isError ? 'Could not load' : undefined}
            to="/profile/phones"
          />
          <SettingsRow
            icon={<DownloadIcon />}
            tone="steps"
            title="Download my data"
            value="Originals and CSV, as a zip"
            download={EXPORT_URL}
            downloadName={exportFileName(me.data?.username, todayIn(s?.timezone))}
          />
          <SettingsRow icon={<ImportIcon />} tone="sleep" title="Import from Garmin" value="Files you exported from Garmin" to="/profile/import-garmin" />
        </SettingsGroup>
      </div>
      <div data-pane="preferences">
        <SettingsGroup title="Preferences">
          <SettingsRow icon={<GlobeIcon />} tone="warn" title="Timezone" value={s ? breakableZone(s.timezone) : settingValue(null)} to="/profile/timezone" />
          <SettingsRow
            icon={<TargetIcon />}
            tone="heart"
            title="Goals"
            value={settingValue(s ? `${fmtInt(s.step_goal)} steps · ${fmtInt(s.intensity_goal_min)} intensity min` : null)}
            to="/profile/goals"
          />
        </SettingsGroup>
      </div>
      <div data-pane="garmin">
        <SettingsGroup title="Garmin Connect">
          <SettingsRow
            icon={<SendIcon />}
            tone="steps"
            title="Send to Garmin"
            value={garmin.data ? GARMIN_STATE_VALUE[garmin.data.state] : garmin.isError ? 'Could not load' : undefined}
            to="/profile/garmin"
          />
        </SettingsGroup>
      </div>
      <div data-pane="about">
        <SettingsGroup title="About">
          <SettingsRow icon={<InfoIcon />} tone="muted" title="Nimrag Connect" value={about.data ? `Server ${about.data.version} · ${LICENCE}` : LICENCE} info />
          <SettingsRow icon={<FilesIcon />} tone="muted" title="Source code" value={SOURCE_URL.replace(/^https?:\/\//, '')} href={SOURCE_URL} />
        </SettingsGroup>
      </div>
    </>
  );
}

/** An admin has a password and a second step, like everyone, and a way back to the admin area. */
function AdminRows() {
  return (
    <>
      <div data-pane="account">
        <SettingsGroup title="Account">
          <SettingsRow icon={<KeyIcon />} tone="accent" title="Password" to="/profile/password" />
          <SettingsRow icon={<ShieldIcon />} tone="energy" title="Two-step login" value="On" to="/profile/two-step" />
        </SettingsGroup>
      </div>
      <div data-pane="account">
        <SettingsGroup title="Administration">
          <SettingsRow icon={<UsersIcon />} tone="sleep" title="Admin area" to="/admin" />
        </SettingsGroup>
      </div>
    </>
  );
}

/**
 * The profile: who is signed in, then the settings as a list of rows, each showing its value and
 * opening its own small page. The avatar in the top bar links here (phone and web). Log out is the
 * last row and asks first.
 */
export function ProfilePage() {
  usePageTitle('Profile');
  const me = useMe();
  const logout = useLogout();
  const location = useLocation();
  const navigate = useNavigate();
  const [search] = useSearchParams();
  const [asking, setAsking] = useState(false);
  const admin = me.data?.role === 'admin';
  const section = sectionFromSearch(`?${search.toString()}`);

  // A page that sent the person here (a changed password) leaves a one-time notice in the navigation state.
  const [notice] = useState(() => {
    const state = location.state as { notice?: unknown } | null;
    return typeof state?.notice === 'string' ? state.notice : null;
  });
  useEffect(() => {
    if (notice && location.state) void navigate(location.pathname + location.search, { replace: true, state: null });
  }, [notice, location.state, location.pathname, location.search, navigate]);

  return (
    <>
      <TopBar variant="back" backTo={admin ? '/admin' : '/'} title="Profile" />
      <div className="page">
        {notice ? (
          <p className="status-msg status-msg--ok" role="status">
            {notice}
          </p>
        ) : null}
        <div className="profile-pane profile-home" data-section={section}>
          <Head name={me.data?.username} host={window.location.host} admin={admin} />
          {admin ? <AdminRows /> : <UserRows />}
          <div data-pane="logout">
            <SettingsGroup title="Log out">
              <SettingsRow icon={<LogoutIcon />} tone="heart" title="Log out" chevron={false} onClick={() => setAsking(true)} />
            </SettingsGroup>
          </div>
        </div>
      </div>
      {asking ? (
        <ConfirmDialog
          title="Log out?"
          consequence="You will need your username, password and a code to log in again on this device. Your other devices stay logged in."
          confirmLabel="Log out"
          pending={logout.isPending}
          onCancel={() => setAsking(false)}
          onConfirm={() => logout.mutate()}
        />
      ) : null}
    </>
  );
}
