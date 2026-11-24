import { useEffect, useRef, useState, type FormEvent } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import type { WorkoutDetail, WorkoutList } from '../../api/generated';
import { Button, Card, CardHeader, ConfirmDialog, SelectField, StatusBanner, TextField, TopBar } from '../../components';
import { errorText } from '../body/common';
import { useDeleteWorkout, useSaveWorkout, useSendWorkout } from './api';
import { sendSentence } from './send';
import { StepEditor } from './StepEditor';
import {
  copyStep,
  draftOf,
  fieldKey,
  emptyDraft,
  fileSteps,
  moved,
  newKey,
  newRepeat,
  newStep,
  SPORTS,
  sportLabel,
  stepCount,
  totalSeconds,
  totalText,
  validate,
  type Errors,
  type NodeDraft,
  type StepDraft,
  type WorkoutDraft,
} from './model';

const NO_ERRORS: Errors = { fields: {} };

/** The draft without its keys: equal while nothing was changed, whatever the keys are. */
function signature(draft: WorkoutDraft): string {
  return JSON.stringify(draft, (key, value: unknown) => (key === 'key' ? undefined : value));
}

interface Props {
  /** The saved workout, kept current by the page (the send state changes while the phone works). Null for a new one. */
  detail: WorkoutDetail | null;
  list: WorkoutList;
}

/**
 * The builder: name and sport, a list of steps and repeat blocks, and the "Watch" card with the send
 * state. The form holds text as typed; `validate` turns it into the body the server stores.
 */
export function WorkoutEditor({ detail, list }: Props) {
  const navigate = useNavigate();
  const location = useLocation();
  const limits = list.limits;
  const workoutId = detail?.id ?? null;

  const [draft, setDraft] = useState<WorkoutDraft>(() => (detail ? draftOf(detail.workout) : emptyDraft()));
  const [baseline, setBaseline] = useState(() => signature(detail ? draftOf(detail.workout) : emptyDraft()));
  // The saved version this form last took in; it changes when the workout is changed elsewhere
  // (the web, or the server's state reaching the phone's copy).
  const savedSignature = detail ? signature(draftOf(detail.workout)) : null;
  const [seenSaved, setSeenSaved] = useState(savedSignature);
  const [submitted, setSubmitted] = useState(false);
  const [focusTick, setFocusTick] = useState(0);
  const [notice, setNotice] = useState<string | null>(() => {
    const state = location.state as { notice?: unknown } | null;
    return typeof state?.notice === 'string' ? state.notice : null;
  });
  const [removing, setRemoving] = useState(false);
  const formRef = useRef<HTMLFormElement>(null);

  const save = useSaveWorkout(workoutId);
  const send = useSendWorkout();
  const del = useDeleteWorkout();
  const busy = save.isPending || send.isPending;

  useEffect(() => {
    if (focusTick === 0) return;
    formRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
  }, [focusTick]);

  const checked = validate(draft, limits);
  const errors = submitted ? checked.errors : NO_ERRORS;
  const dirty = signature(draft) !== baseline;
  // A newer saved version: the form shows it unless the person has unsaved changes (those win,
  // and saving them replaces it). The same content as the form (its own save) changes nothing.
  if (detail && savedSignature !== seenSaved) {
    setSeenSaved(savedSignature);
    if (!dirty && savedSignature !== baseline) {
      setDraft(draftOf(detail.workout));
      setBaseline(savedSignature ?? '');
    }
  }
  const watch = list.watch_send;
  const used = fileSteps(draft.nodes);
  const room = limits.max_steps - used;

  // ---- editing ----

  const edit = (fn: (d: WorkoutDraft) => WorkoutDraft) => {
    setNotice(null);
    setDraft(fn);
  };
  const setNode = (index: number, fn: (n: NodeDraft) => NodeDraft) =>
    edit((d) => ({ ...d, nodes: d.nodes.map((n, i) => (i === index ? fn(n) : n)) }));
  const patchStep = (index: number, patch: Partial<StepDraft>) =>
    setNode(index, (n) => (n.type === 'step' ? { ...n, step: { ...n.step, ...patch } } : n));
  const patchInner = (index: number, inner: number, patch: Partial<StepDraft>) =>
    setNode(index, (n) => (n.type === 'repeat' ? { ...n, steps: n.steps.map((s, j) => (j === inner ? { ...s, ...patch } : s)) } : n));
  const setInner = (index: number, fn: (steps: StepDraft[]) => StepDraft[]) =>
    setNode(index, (n) => (n.type === 'repeat' ? { ...n, steps: fn(n.steps) } : n));

  // ---- saving, sending, deleting ----

  const submit = async (sendAfter: boolean) => {
    setSubmitted(true);
    setNotice(null);
    save.reset();
    send.reset();
    const result = validate(draft, limits);
    if (!result.body) {
      setFocusTick((t) => t + 1);
      return;
    }
    const signed = signature(draft);
    try {
      const saved = await save.mutateAsync(result.body);
      setBaseline(signed);
      let text = 'Workout saved.';
      if (sendAfter) {
        try {
          await send.mutateAsync({ id: saved.id, send: true });
          text = 'Workout saved. It goes to the watch when the phone is connected to it.';
        } catch (error) {
          text = `Workout saved, but it could not be sent to the watch. ${errorText(error)}`;
        }
      }
      if (workoutId === null) void navigate(`/workouts/${saved.id}`, { replace: true, state: { notice: text } });
      else setNotice(text);
    } catch {
      // The failed save is shown under the form (save.error); the draft stays as typed.
    }
  };

  const changeSend = (queue: boolean) => {
    if (workoutId === null) return;
    setNotice(null);
    send.mutate(
      { id: workoutId, send: queue },
      { onSuccess: () => setNotice(queue ? 'It goes to the watch when the phone is connected to it.' : 'Sending cancelled.') },
    );
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    void submit(false);
  };

  const total = checked.body ? totalText(totalSeconds(checked.body.nodes)) : null;
  const count = stepCount(draft.nodes);
  const title = detail ? detail.workout.name : 'New workout';

  return (
    <>
      <TopBar variant="back" backTo="/workouts" backLabel="Back to workouts" title={title} subtitle={detail ? sportLabel(detail.workout.sport) : 'Build it here, send it to the watch'} />
      <form ref={formRef} onSubmit={onSubmit} noValidate>
        <div className="page">
          <div className="wk-layout">
            <div className="wk-main">
              <Card padding="lg" aria-labelledby="wk-details">
                <CardHeader title="Details" id="wk-details" />
                <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
                  <TextField
                    label="Name"
                    name="name"
                    autoComplete="off"
                    value={draft.name}
                    onChange={(e) => edit((d) => ({ ...d, name: e.target.value }))}
                    aria-invalid={errors.name ? true : undefined}
                    hint={errors.name ?? 'Shown on the watch. Up to 63 letters.'}
                  />
                  <SelectField label="Sport" value={draft.sport} onChange={(e) => edit((d) => ({ ...d, sport: e.target.value as WorkoutDraft['sport'] }))}>
                    {SPORTS.map((s) => (
                      <option key={s.value} value={s.value}>
                        {s.label}
                      </option>
                    ))}
                  </SelectField>
                  <TextField
                    label="Note (optional)"
                    name="note"
                    autoComplete="off"
                    value={draft.note}
                    onChange={(e) => edit((d) => ({ ...d, note: e.target.value }))}
                    aria-invalid={errors.note ? true : undefined}
                    hint={errors.note ?? 'Shown with the workout on the watch.'}
                  />
                </div>
              </Card>

              <Card padding="lg" aria-labelledby="wk-steps">
                <CardHeader
                  title="Steps"
                  id="wk-steps"
                  aside={`${count} ${count === 1 ? 'step' : 'steps'}${total ? ` · ${total}` : ''}`}
                />
                <p className="wk-note">
                  {room >= 0 ? `Room for ${room} more (a repeat block counts one extra). ` : ''}
                  Steps run from top to bottom.
                </p>
                {errors.form ? (
                  <p className="wk-error" role="alert">
                    {errors.form}
                  </p>
                ) : null}
                <ol className="wk-steps">
                  {draft.nodes.map((node, i) => (
                    <NodeEditor
                      key={node.key}
                      node={node}
                      index={i}
                      last={i === draft.nodes.length - 1}
                      errors={errors}
                      room={room}
                      limits={limits}
                      onPatchStep={(patch) => patchStep(i, patch)}
                      onPatchInner={(j, patch) => patchInner(i, j, patch)}
                      onCount={(count) => setNode(i, (n) => (n.type === 'repeat' ? { ...n, count } : n))}
                      onInner={(fn) => setInner(i, fn)}
                      onMove={(by) => edit((d) => ({ ...d, nodes: moved(d.nodes, i, i + by) }))}
                      onCopy={() =>
                        edit((d) => {
                          const n = d.nodes[i];
                          if (!n || n.type !== 'step') return d;
                          const nodes = [...d.nodes];
                          nodes.splice(i + 1, 0, { type: 'step', key: newKey(), step: copyStep(n.step) });
                          return { ...d, nodes };
                        })
                      }
                      onRemove={() => edit((d) => ({ ...d, nodes: d.nodes.filter((_, k) => k !== i) }))}
                    />
                  ))}
                </ol>
                <div className="wk-add">
                  <Button
                    variant="dashed"
                    disabled={room < 1}
                    onClick={() => edit((d) => ({ ...d, nodes: [...d.nodes, { type: 'step', key: newKey(), step: newStep() }] }))}
                  >
                    Add step
                  </Button>
                  <Button variant="dashed" disabled={room < 3} onClick={() => edit((d) => ({ ...d, nodes: [...d.nodes, newRepeat()] }))}>
                    Add repeat block
                  </Button>
                </div>
                {room < 3 ? <p className="wk-note">The watch holds at most {limits.max_steps} steps in one workout.</p> : null}
              </Card>
            </div>

            <div className="wk-side">
              <Card padding="lg" aria-labelledby="wk-watch">
                <CardHeader title="Watch" id="wk-watch" />
                {!watch.available ? (
                  <StatusBanner tone="info">{watch.reason ?? 'This server cannot send workouts to the watch.'}</StatusBanner>
                ) : null}
                {detail ? (
                  <p className={`wk-state${detail.send.state === 'failed' ? ' wk-state--failed' : ''}`} role="status">
                    {sendSentence(detail.send)}
                  </p>
                ) : (
                  <p className="wk-note">Save the workout first. Then it can be sent to the watch.</p>
                )}
                {detail && watch.available ? (
                  <>
                    <Button
                      variant={detail.send.state === 'queued' ? 'secondary' : 'primary'}
                      size="lg"
                      block
                      disabled={busy || (detail.send.state !== 'queued' && dirty)}
                      aria-describedby={dirty ? 'wk-dirty' : undefined}
                      onClick={() => changeSend(detail.send.state !== 'queued')}
                    >
                      {detail.send.state === 'queued'
                        ? 'Cancel sending'
                        : detail.send.state === 'sent'
                          ? 'Send again'
                          : detail.send.state === 'failed'
                            ? 'Try again'
                            : 'Send to watch'}
                    </Button>
                    {dirty ? (
                      <p className="wk-note" id="wk-dirty">
                        You have unsaved changes. Save first: the watch gets the saved workout.
                      </p>
                    ) : null}
                  </>
                ) : null}
                {send.isError ? (
                  <p className="wk-error" role="alert">
                    {errorText(send.error)}
                  </p>
                ) : null}
              </Card>

              {detail ? (
                <Button size="lg" block onClick={() => setRemoving(true)}>
                  Delete workout
                </Button>
              ) : null}
            </div>
          </div>

          {notice ? (
            <p className="status-msg status-msg--ok" role="status">
              {notice}
            </p>
          ) : null}
          {save.isError ? (
            <p className="status-msg status-msg--error" role="alert">
              The workout was not saved. {errorText(save.error)}
            </p>
          ) : null}
          {submitted && !checked.body && !save.isError ? (
            <p className="status-msg status-msg--error" role="alert">
              The workout was not saved: some details need attention. Fix what is marked and save again.
            </p>
          ) : null}
        </div>

        <div className={watch.available ? 'footer-bar' : 'footer-bar footer-bar--single'}>
          <Button type="submit" size="lg" variant={watch.available ? 'secondary' : 'primary'} block disabled={busy} aria-busy={save.isPending}>
            {save.isPending && !send.isPending ? 'Saving' : 'Save workout'}
          </Button>
          {watch.available ? (
            <Button size="lg" variant="primary" block disabled={busy} aria-busy={send.isPending} onClick={() => void submit(true)}>
              {send.isPending ? 'Sending' : 'Save and send to watch'}
            </Button>
          ) : null}
        </div>
      </form>

      {removing && detail ? (
        <ConfirmDialog
          title="Delete this workout?"
          consequence="It is removed from this server. A copy that is already on the watch stays there until you delete it on the watch."
          confirmLabel="Delete workout"
          pending={del.isPending}
          error={del.isError ? errorText(del.error) : undefined}
          onCancel={() => {
            del.reset();
            setRemoving(false);
          }}
          onConfirm={() => del.mutate(detail.id, { onSuccess: () => void navigate('/workouts', { replace: true }) })}
        />
      ) : null}
    </>
  );
}

interface NodeProps {
  node: NodeDraft;
  index: number;
  last: boolean;
  errors: Errors;
  /** Steps still allowed in the file. */
  room: number;
  limits: WorkoutList['limits'];
  onPatchStep: (patch: Partial<StepDraft>) => void;
  onPatchInner: (inner: number, patch: Partial<StepDraft>) => void;
  onCount: (count: string) => void;
  onInner: (fn: (steps: StepDraft[]) => StepDraft[]) => void;
  onMove: (by: -1 | 1) => void;
  onCopy: () => void;
  onRemove: () => void;
}

function NodeEditor({ node, index, last, errors, room, limits, onPatchStep, onPatchInner, onCount, onInner, onMove, onCopy, onRemove }: NodeProps) {
  const number = index + 1;
  if (node.type === 'step') {
    return (
      <StepEditor
        step={node.step}
        label={`Step ${number}`}
        errors={errors}
        onChange={onPatchStep}
        onMove={onMove}
        canMoveUp={index > 0}
        canMoveDown={!last}
        onCopy={onCopy}
        canCopy={room >= 1}
        onRemove={onRemove}
        canRemove
      />
    );
  }

  const countError = errors.fields[fieldKey(node.key, 'count')];
  const stepsError = errors.fields[fieldKey(node.key, 'steps')];
  const label = `Repeat block ${number}`;
  return (
    <Card as="li" padding="lg">
      <fieldset className="wk-fieldset">
        <legend className="wk-legend">{label}</legend>
        <TextField
          label="Repeat times"
          name={`${node.key}-count`}
          inputMode="numeric"
          autoComplete="off"
          value={node.count}
          onChange={(e) => onCount(e.target.value)}
          aria-invalid={countError ? true : undefined}
          hint={countError ?? `${limits.repeat_min} to ${limits.repeat_max} times.`}
        />
        {stepsError ? <p className="wk-error">{stepsError}</p> : null}
        <ol className="wk-steps wk-steps--nested">
          {node.steps.map((s, j) => (
            <StepEditor
              key={s.key}
              step={s}
              label={`Step ${number}.${j + 1}`}
              errors={errors}
              onChange={(patch) => onPatchInner(j, patch)}
              onMove={(by) => onInner((steps) => moved(steps, j, j + by))}
              canMoveUp={j > 0}
              canMoveDown={j < node.steps.length - 1}
              onCopy={() =>
                onInner((steps) => {
                  const next = [...steps];
                  next.splice(j + 1, 0, copyStep(s));
                  return next;
                })
              }
              canCopy={room >= 1 && node.steps.length < limits.max_block_steps}
              onRemove={() => onInner((steps) => steps.filter((_, k) => k !== j))}
              canRemove={node.steps.length > 1}
            />
          ))}
        </ol>
        <Button
          variant="dashed"
          disabled={room < 1 || node.steps.length >= limits.max_block_steps}
          onClick={() => onInner((steps) => [...steps, newStep()])}
        >
          {`Add step to ${label.toLowerCase()}`}
        </Button>
        <div className="wk-tools" role="group" aria-label={`${label} actions`}>
          <Button aria-label={`Move ${label.toLowerCase()} up`} disabled={index === 0} onClick={() => onMove(-1)}>
            Up
          </Button>
          <Button aria-label={`Move ${label.toLowerCase()} down`} disabled={last} onClick={() => onMove(1)}>
            Down
          </Button>
          <Button aria-label={`Remove ${label.toLowerCase()}`} onClick={onRemove}>
            Remove
          </Button>
        </div>
      </fieldset>
    </Card>
  );
}
