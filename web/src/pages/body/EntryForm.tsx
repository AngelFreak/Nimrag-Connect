import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState, type FormEvent } from 'react';
import { api } from '../../api/client';
import type { BodyEntry, NewBodyEntry } from '../../api/generated';
import { Button, Card, CardHeader, TextField } from '../../components';
import { validateEntry, type EntryForm as Values, type FormErrors } from './body';
import { errorText } from './common';

interface Props {
  /** The owner's today (YYYY-MM-DD): default and latest allowed date. */
  today: string;
  onDone: () => void;
  onCancel: () => void;
}

/** Add-entry form: weight (kg), body fat (%), waist (cm) and a date. At least one measurement is needed. */
export function EntryForm({ today, onDone, onCancel }: Props) {
  const client = useQueryClient();
  const [values, setValues] = useState<Values>({ date: today, weight: '', fat: '', waist: '' });
  const [errors, setErrors] = useState<FormErrors>({});

  const weight = useRef<HTMLInputElement>(null);
  useEffect(() => {
    weight.current?.focus();
  }, []);

  const save = useMutation({
    mutationFn: (body: NewBodyEntry) => api.post<BodyEntry>('/api/body', body),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ['body'] });
      await client.invalidateQueries({ queryKey: ['today'] });
      onDone();
    },
  });

  const set = (key: keyof Values) => (e: { target: { value: string } }) => setValues((v) => ({ ...v, [key]: e.target.value }));

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const { errors: found, payload } = validateEntry(values, today);
    setErrors(found);
    if (payload === null) return;
    save.mutate(payload);
  };

  return (
    <Card padding="lg" aria-labelledby="entry-form-title">
      <CardHeader title="Add entry" id="entry-form-title" />
      <form onSubmit={submit} noValidate style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        <TextField
          ref={weight}
          label="Weight (kg)"
          name="weight"
          inputMode="decimal"
          autoComplete="off"
          value={values.weight}
          onChange={set('weight')}
          aria-invalid={errors.weight ? true : undefined}
          hint={errors.weight}
        />
        <TextField
          label="Body fat (%)"
          name="body_fat"
          inputMode="decimal"
          autoComplete="off"
          value={values.fat}
          onChange={set('fat')}
          aria-invalid={errors.fat ? true : undefined}
          hint={errors.fat}
        />
        <TextField
          label="Waist (cm)"
          name="waist"
          inputMode="decimal"
          autoComplete="off"
          value={values.waist}
          onChange={set('waist')}
          aria-invalid={errors.waist ? true : undefined}
          hint={errors.waist}
        />
        <TextField label="Date" name="date" type="date" max={today} value={values.date} onChange={set('date')} aria-invalid={errors.date ? true : undefined} hint={errors.date} />
        {errors.form ? (
          <p role="alert" className="field__hint" style={{ color: 'var(--heart)' }}>
            {errors.form}
          </p>
        ) : null}
        {save.isError ? (
          <p role="alert" className="field__hint" style={{ color: 'var(--heart)' }}>
            The entry was not saved. {errorText(save.error)}
          </p>
        ) : null}
        <div className="grid-2">
          <Button type="button" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={save.isPending}>
            {save.isPending ? 'Saving' : 'Save entry'}
          </Button>
        </div>
      </form>
    </Card>
  );
}
