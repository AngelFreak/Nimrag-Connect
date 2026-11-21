import { Button, Card, SelectField, TextField } from '../../components';
import {
  DURATIONS,
  fieldKey,
  INTENSITIES,
  TARGETS,
  type DurationKind,
  type Errors,
  type StepDraft,
  type TargetKind,
} from './model';

interface StepProps {
  step: StepDraft;
  /** "Step 2" or "Step 2.1": the name every button and the group carry. */
  label: string;
  errors: Errors;
  onChange: (patch: Partial<StepDraft>) => void;
  onMove: (by: -1 | 1) => void;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onCopy: () => void;
  canCopy: boolean;
  onRemove: () => void;
  canRemove: boolean;
}

const DURATION_FIELD: Record<Exclude<DurationKind, 'open'>, { label: string; hint: string; placeholder: string; mode: 'text' | 'numeric' }> = {
  time: { label: 'Time', hint: 'Minutes and seconds, like 5:00. A plain number is minutes.', placeholder: '5:00', mode: 'text' },
  distance: { label: 'Distance (m)', hint: 'Whole metres, like 400.', placeholder: '400', mode: 'numeric' },
  heart_rate_below: { label: 'Heart rate (bpm)', hint: 'The step ends when your heart rate falls below this.', placeholder: '120', mode: 'numeric' },
  heart_rate_above: { label: 'Heart rate (bpm)', hint: 'The step ends when your heart rate rises above this.', placeholder: '160', mode: 'numeric' },
};

/** What the two target fields are called and how they read, by target kind. */
const TARGET_FIELDS: Partial<Record<TargetKind, { a: string; b: string; hintA?: string; placeholderA: string; placeholderB: string; mode: 'text' | 'numeric' | 'decimal' }>> = {
  heart_rate_range: { a: 'Lowest heart rate (bpm)', b: 'Highest heart rate (bpm)', placeholderA: '120', placeholderB: '140', mode: 'numeric' },
  pace: { a: 'Fastest pace (min/km)', b: 'Slowest pace (min/km)', hintA: 'Minutes and seconds per km, like 5:30.', placeholderA: '5:30', placeholderB: '6:15', mode: 'text' },
  speed: { a: 'Lowest speed (km/h)', b: 'Highest speed (km/h)', placeholderA: '9', placeholderB: '11', mode: 'decimal' },
  cadence: { a: 'Lowest cadence', b: 'Highest cadence', hintA: 'Steps or revolutions per minute.', placeholderA: '160', placeholderB: '175', mode: 'numeric' },
};

/** One step: what it is, how it ends, what to aim for, and the buttons that move, copy and remove it. */
export function StepEditor({ step, label, errors, onChange, onMove, canMoveUp, canMoveDown, onCopy, canCopy, onRemove, canRemove }: StepProps) {
  const duration = step.durationKind === 'open' ? null : DURATION_FIELD[step.durationKind];
  const durationError = errors.fields[fieldKey(step.key, 'duration')];
  const targetError = errors.fields[fieldKey(step.key, 'target')];
  const noteError = errors.fields[fieldKey(step.key, 'note')];
  const target = TARGET_FIELDS[step.targetKind];

  return (
    <Card as="li" padding="lg">
      <fieldset className="wk-fieldset">
        <legend className="wk-legend">{label}</legend>
        <div className="wk-row">
          <SelectField label="Type" value={step.intensity} onChange={(e) => onChange({ intensity: e.target.value as StepDraft['intensity'] })}>
            {INTENSITIES.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </SelectField>
          <SelectField
            label="Ends"
            value={step.durationKind}
            onChange={(e) => onChange({ durationKind: e.target.value as DurationKind, duration: '' })}
          >
            {DURATIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </SelectField>
        </div>
        {duration ? (
          <TextField
            label={duration.label}
            name={`${step.key}-duration`}
            inputMode={duration.mode}
            placeholder={duration.placeholder}
            autoComplete="off"
            value={step.duration}
            onChange={(e) => onChange({ duration: e.target.value })}
            aria-invalid={durationError ? true : undefined}
            hint={durationError ?? duration.hint}
          />
        ) : null}

        <SelectField
          label="Target"
          value={step.targetKind}
          onChange={(e) => onChange({ targetKind: e.target.value as TargetKind, targetA: '', targetB: '' })}
        >
          {TARGETS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </SelectField>
        {step.targetKind === 'heart_rate_zone' ? (
          <SelectField
            label="Zone"
            value={step.targetA}
            onChange={(e) => onChange({ targetA: e.target.value })}
            aria-invalid={targetError ? true : undefined}
            hint={targetError}
          >
            <option value="">Choose a zone</option>
            {[1, 2, 3, 4, 5].map((z) => (
              <option key={z} value={String(z)}>
                Zone {z}
              </option>
            ))}
          </SelectField>
        ) : null}
        {target ? (
          <>
            <div className="wk-row">
              <TextField
                label={target.a}
                name={`${step.key}-target-a`}
                inputMode={target.mode}
                autoComplete="off"
                placeholder={target.placeholderA}
                value={step.targetA}
                onChange={(e) => onChange({ targetA: e.target.value })}
                aria-invalid={targetError ? true : undefined}
              />
              <TextField
                label={target.b}
                name={`${step.key}-target-b`}
                inputMode={target.mode}
                autoComplete="off"
                placeholder={target.placeholderB}
                value={step.targetB}
                onChange={(e) => onChange({ targetB: e.target.value })}
                aria-invalid={targetError ? true : undefined}
              />
            </div>
            {targetError ? (
              <p className="wk-error">{targetError}</p>
            ) : target.hintA ? (
              <p className="wk-note">{target.hintA}</p>
            ) : null}
          </>
        ) : null}

        <TextField
          label="Note (optional)"
          name={`${step.key}-note`}
          autoComplete="off"
          value={step.note}
          onChange={(e) => onChange({ note: e.target.value })}
          aria-invalid={noteError ? true : undefined}
          hint={noteError ?? 'Shown on the watch during the step.'}
        />

        <div className="wk-tools" role="group" aria-label={`${label} actions`}>
          <Button aria-label={`Move ${label.toLowerCase()} up`} disabled={!canMoveUp} onClick={() => onMove(-1)}>
            Up
          </Button>
          <Button aria-label={`Move ${label.toLowerCase()} down`} disabled={!canMoveDown} onClick={() => onMove(1)}>
            Down
          </Button>
          <Button aria-label={`Copy ${label.toLowerCase()}`} disabled={!canCopy} onClick={onCopy}>
            Copy
          </Button>
          <Button aria-label={`Remove ${label.toLowerCase()}`} disabled={!canRemove} onClick={onRemove}>
            Remove
          </Button>
        </div>
      </fieldset>
    </Card>
  );
}
