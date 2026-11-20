import type { SettingsPatch, SettingsResponse } from '../../api/generated';

export const STEP_GOAL_MAX = 200_000;
export const INTENSITY_GOAL_MAX = 10_080;

export interface GoalsForm {
  timezone: string;
  steps: string;
  intensity: string;
}

export interface GoalsErrors {
  timezone?: string;
  steps?: string;
  intensity?: string;
}

export function formFrom(s: SettingsResponse): GoalsForm {
  return { timezone: s.timezone, steps: String(s.step_goal), intensity: String(s.intensity_goal_min) };
}

function validZone(name: string): boolean {
  try {
    new Intl.DateTimeFormat('en-GB', { timeZone: name });
    return true;
  } catch {
    return false;
  }
}

const WHOLE = /^\d+$/;

/**
 * Checks the form and returns the PATCH body: only the fields that changed. `patch` is null when
 * something is invalid or nothing changed.
 */
export function validateGoals(form: GoalsForm, saved: SettingsResponse): { errors: GoalsErrors; patch: SettingsPatch | null } {
  const errors: GoalsErrors = {};
  const patch: SettingsPatch = {};
  const tz = form.timezone.trim();
  if (!tz || !validZone(tz)) errors.timezone = 'Use an IANA name such as Europe/Copenhagen.';
  else if (tz !== saved.timezone) patch.timezone = tz;

  const steps = form.steps.trim();
  if (!WHOLE.test(steps) || Number(steps) < 1 || Number(steps) > STEP_GOAL_MAX) errors.steps = `Enter a whole number from 1 to ${STEP_GOAL_MAX.toLocaleString('en-GB')}.`;
  else if (Number(steps) !== saved.step_goal) patch.step_goal = Number(steps);

  const intensity = form.intensity.trim();
  if (!WHOLE.test(intensity) || Number(intensity) < 1 || Number(intensity) > INTENSITY_GOAL_MAX)
    errors.intensity = `Enter a whole number of minutes from 1 to ${INTENSITY_GOAL_MAX.toLocaleString('en-GB')}.`;
  else if (Number(intensity) !== saved.intensity_goal_min) patch.intensity_goal_min = Number(intensity);

  const bad = Object.keys(errors).length > 0;
  return { errors, patch: bad || Object.keys(patch).length === 0 ? null : patch };
}
