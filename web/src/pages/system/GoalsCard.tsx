import { useState, type FormEvent } from 'react';
import type { SettingsPatch, SettingsResponse } from '../../api/generated';
import { Button, Card, TextField } from '../../components';
import { errorMessage } from './errors';
import { useSaveSettings } from './queries';

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

function zoneNames(): string[] {
  try {
    return (Intl as unknown as { supportedValuesOf?: (k: string) => string[] }).supportedValuesOf?.('timeZone') ?? [];
  } catch {
    return [];
  }
}

/** Settings, "Timezone and goals": GET and PATCH /api/settings. The form is keyed to the saved values by its parent. */
export function GoalsCard({ settings }: { settings: SettingsResponse }) {
  const save = useSaveSettings();
  const [form, setForm] = useState<GoalsForm>(() => formFrom(settings));
  const [errors, setErrors] = useState<GoalsErrors>({});
  const [saved, setSaved] = useState(false);
  const check = validateGoals(form, settings);
  const changed = check.patch !== null || Object.keys(check.errors).length > 0;

  const set = (key: keyof GoalsForm) => (e: { target: { value: string } }) => {
    setForm((f) => ({ ...f, [key]: e.target.value }));
    setSaved(false);
    save.reset();
  };

  function submit(e: FormEvent) {
    e.preventDefault();
    const { errors: found, patch } = validateGoals(form, settings);
    setErrors(found);
    if (patch === null) return;
    save.mutate(patch, { onSuccess: (s) => { setForm(formFrom(s)); setSaved(true); } });
  }

  return (
    <Card padding="lg" aria-label="Timezone and goals">
      <form onSubmit={submit} noValidate className="form">
        <div className="form__group">
          <h3 className="form__group-title">Timezone</h3>
          <TextField
            label="Timezone (IANA name)"
            name="timezone"
            list="timezone-names"
            autoComplete="off"
            autoCapitalize="off"
            spellCheck={false}
            value={form.timezone}
            onChange={set('timezone')}
            aria-invalid={errors.timezone ? true : undefined}
            hint={errors.timezone ?? 'Days are local days in this timezone.'}
          />
          <datalist id="timezone-names">
            {zoneNames().map((z) => (
              <option key={z} value={z} />
            ))}
          </datalist>
        </div>
        <div className="form__group">
          <h3 className="form__group-title">Goals</h3>
          <div className="form__row">
            <TextField
              label="Daily step goal"
              name="step_goal"
              type="text"
              inputMode="numeric"
              autoComplete="off"
              value={form.steps}
              onChange={set('steps')}
              aria-invalid={errors.steps ? true : undefined}
              hint={errors.steps}
            />
            <TextField
              label="Weekly intensity minutes goal"
              name="intensity_goal_min"
              type="text"
              inputMode="numeric"
              autoComplete="off"
              value={form.intensity}
              onChange={set('intensity')}
              aria-invalid={errors.intensity ? true : undefined}
              hint={errors.intensity}
            />
          </div>
        </div>
        <Button type="submit" size="lg" variant="primary" disabled={!changed || save.isPending} aria-busy={save.isPending}>
          {save.isPending ? 'Saving' : 'Save settings'}
        </Button>
        {saved ? (
          <p className="status-msg status-msg--ok" role="status">
            Saved.
          </p>
        ) : null}
        {save.isError ? (
          <p className="status-msg status-msg--error" role="alert">
            {errorMessage(save.error)}
          </p>
        ) : null}
      </form>
    </Card>
  );
}
