import { Link } from 'react-router-dom';
import type { SettingsResponse, Today } from '../../api/generated';
import { Card, CardHeader, DataList } from '../../components';
import { fmtShortDay } from '../../lib/dates';
import { fmtFixed, fmtInt, isMissing } from '../../lib/format';
import { isUtcZone, suggestedZone } from '../../lib/timezone';
import { DEFAULT_HOME_CARDS, GlanceCard, IN_FOCUS_CARDS, visibleCards } from './cards';
import { fmtSeconds, kindLabel, signed, unit } from './format';

function join(...parts: (string | null | undefined)[]): string | null {
  const present = parts.filter((p): p is string => typeof p === 'string' && p.length > 0);
  return present.length > 0 ? present.join(' · ') : null;
}

/**
 * A server on UTC cuts the owner's days at the wrong hour. When the account zone is UTC and the
 * browser's is not, say so and link to where it is set. Nothing is shown otherwise.
 */
export function TimezoneNotice({ timezone }: { timezone: string }) {
  if (!isUtcZone(timezone) || suggestedZone(timezone) === null) return null;
  return (
    <p className="today-note today-tz" role="note">
      Your days follow UTC time. <Link to="/profile/timezone">Set your timezone</Link>
    </p>
  );
}

/** "At a glance": the small cards in the owner's order, hidden ones left out. */
export function AtAGlance({ today, settings }: { today: Today; settings: SettingsResponse | undefined }) {
  const ids = visibleCards(settings?.home_cards ?? DEFAULT_HOME_CARDS).filter((id) => !IN_FOCUS_CARDS.includes(id));
  return (
    <section className="today-section" aria-labelledby="glance-title">
      <h2 className="section-label" id="glance-title">
        At a glance
      </h2>
      {ids.length === 0 ? (
        <p className="glance-empty">Every card is hidden. Use Edit home below to bring some back.</p>
      ) : (
        <div className="glance-grid">
          {ids.map((id) => (
            <GlanceCard key={id} id={id} today={today} />
          ))}
        </div>
      )}
    </section>
  );
}

export function YesterdayCard({ today }: { today: Today }) {
  const y = today.yesterday;
  const flows =
    isMissing(y.energy_charged) && isMissing(y.energy_drained)
      ? null
      : `${isMissing(y.energy_charged) ? 'unavailable' : signed(y.energy_charged)} / ${isMissing(y.energy_drained) ? 'unavailable' : signed(-y.energy_drained)}`;
  const hr = join(isMissing(y.resting_hr_bpm) ? null : `rest ${fmtInt(y.resting_hr_bpm)}`, isMissing(y.hr_high_bpm) ? null : `high ${fmtInt(y.hr_high_bpm)}`);
  return (
    <Card as="section" aria-labelledby="yesterday-title">
      <CardHeader title="Yesterday" id="yesterday-title" aside={fmtShortDay(y.date)} />
      <DataList
        hideMissing
        empty="Shows after your watch syncs a day of data."
        items={[
          { label: 'Energy in / out', value: flows },
          { label: 'Steps', value: fmtInt(y.steps) },
          { label: 'Sleep', value: join(isMissing(y.sleep_score) ? null : fmtInt(y.sleep_score), fmtSeconds(y.sleep_duration_s)) },
          { label: 'Heart rate', value: hr },
          { label: 'Avg stress', value: fmtInt(y.stress_avg) },
          { label: 'HRV, 7-day avg', value: unit(y.hrv_7d_avg_ms, 'ms') },
          { label: 'Intensity min', value: fmtInt(y.intensity_minutes) },
        ]}
      />
    </Card>
  );
}

export function Last7DaysCard({ today }: { today: Today }) {
  const w = today.last_7_days;
  const kinds = w.activities_by_kind.map((k) => `${k.count} ${kindLabel(k.kind).toLowerCase()}`).join(', ');
  return (
    <Card as="section" aria-labelledby="last7-title">
      <CardHeader title="Last 7 days" id="last7-title" aside={`${fmtShortDay(w.from)} to ${fmtShortDay(w.to)}`} />
      <DataList
        hideMissing
        empty="Shows after your watch syncs a few days."
        items={[
          { label: 'Activities', value: w.activity_count === 0 ? 'None' : join(String(w.activity_count), kinds) },
          { label: 'Steps per day', value: fmtInt(w.avg_steps) },
          { label: 'Sleep', value: join(isMissing(w.avg_sleep_score) ? null : `score ${fmtInt(w.avg_sleep_score)}`, fmtSeconds(w.avg_sleep_duration_s)) },
          { label: 'Resting HR', value: unit(w.avg_resting_hr_bpm, 'bpm') },
          { label: 'Avg stress', value: isMissing(w.avg_stress) ? null : fmtFixed(w.avg_stress, 0) },
        ]}
      />
    </Card>
  );
}
