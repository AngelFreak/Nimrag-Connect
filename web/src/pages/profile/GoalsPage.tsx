import { useState, type FormEvent } from 'react';
import type { SettingsResponse } from '../../api/generated';
import { Button, Card, ErrorState, SelectField, Skeleton, TextField } from '../../components';
import { useSettings } from '../today/queries';
import { errorMessage } from '../system/errors';
import { useSaveSettings } from '../system/queries';
import { SubPage } from './common';
import { formFrom, SLEEP_NEED_CHOICES, sleepNeedLabel, validateGoals, type GoalsErrors } from './goals';

function GoalsForm({ settings }: { settings: SettingsResponse }) {
  const save = useSaveSettings();
  const [form, setForm] = useState(() => formFrom(settings));
  const [errors, setErrors] = useState<GoalsErrors>({});
  const [saved, setSaved] = useState(false);
  const check = validateGoals(form, settings);
  const changed = check.patch !== null || Object.keys(check.errors).length > 0;

  const set = (key: 'steps' | 'intensity' | 'sleepNeed') => (e: { target: { value: string } }) => {
    setForm((f) => ({ ...f, [key]: e.target.value }));
    setSaved(false);
    save.reset();
  };

  function submit(e: FormEvent) {
    e.preventDefault();
    const { errors: found, patch } = validateGoals(form, settings);
    setErrors(found);
    if (patch === null) return;
    save.mutate(patch, {
      onSuccess: (s) => {
        setForm(formFrom(s));
        setSaved(true);
      },
    });
  }

  return (
    <Card padding="lg">
      <form onSubmit={submit} noValidate className="form" aria-label="Goals">
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
        <SelectField
          label="Sleep need"
          name="sleep_need_min"
          value={form.sleepNeed}
          onChange={set('sleepNeed')}
          aria-invalid={errors.sleepNeed ? true : undefined}
          hint={errors.sleepNeed ?? 'The sleep page rates the length of a night against this.'}
        >
          {SLEEP_NEED_CHOICES.map((m) => (
            <option key={m} value={String(m)}>
              {sleepNeedLabel(m)}
            </option>
          ))}
        </SelectField>
        <Button type="submit" size="lg" variant="primary" disabled={!changed || save.isPending} aria-busy={save.isPending}>
          {save.isPending ? 'Saving' : 'Save goals'}
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

/** Profile, Goals: GET and PATCH /api/settings (daily steps, weekly intensity minutes, sleep need). */
export function GoalsPage() {
  const settings = useSettings();
  return (
    <SubPage title="Goals" backTo="/profile?section=preferences">
      <p className="subpage__lede">Your goals fill the progress bars on Today and Health.</p>
      {settings.isPending ? (
        <Card padding="lg" aria-label="Loading the goals">
          <Skeleton height={20} />
          <Skeleton height={48} lines={3} />
        </Card>
      ) : null}
      {settings.isError && !settings.data ? (
        <Card padding="lg">
          <ErrorState title="Could not load the settings" action={<Button onClick={() => void settings.refetch()}>Try again</Button>}>
            {errorMessage(settings.error)}
          </ErrorState>
        </Card>
      ) : null}
      {settings.data ? <GoalsForm settings={settings.data} /> : null}
    </SubPage>
  );
}
