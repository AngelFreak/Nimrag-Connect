import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import { Link } from 'react-router-dom';
import type { ExerciseGroup } from '../../api/generated/ExerciseGroup';
import type { StrengthResponse } from '../../api/generated';
import type { StrengthSet } from '../../api/generated/StrengthSet';
import { Button, Card, CardHeader, SeriesChart, StatTile, Unavailable } from '../../components';
import { fmtShortDay } from '../../lib/dates';
import { fmtClock, fmtInt } from '../../lib/format';
import { useIsDesktop } from '../../lib/hooks';
import { useSaveStrength } from './api';
import { ChartBlock, DetailHeader, DetailLayout, SideCard, SideFacts } from './DetailParts';
import { fmtBpm, fmtKg, fmtMinutes, longWhen } from './format';
import { Tabs } from './Tabs';
import { errorText } from './PageStates';
import { spread, timedToMinutes, timeLabels } from './series';
import { StrengthCompare } from './StrengthCompare';
import {
  buildPatch,
  differsFromWatch,
  draftOf,
  draftsOf,
  loadText,
  repsText,
  validateAll,
  watchDraftOf,
  type Drafts,
  type SetDraft,
  type SetErrors,
} from './strengthModel';
import './activities.css';

function UndoIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
      <path d="M9 14L4 9l5-5" />
      <path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" />
    </svg>
  );
}

const FORM_ID = 'strength-edit-form';

/** Read-only exercise list, as drawn in Strength.dc.html. */
function ExerciseList({ groups }: { groups: readonly ExerciseGroup[] }) {
  return (
    <Card padding="list">
      <h2 className="sr-only">Exercises</h2>
      <ul>
        {groups.map((g, i) => {
          const load = loadText(g.sets);
          const reps = repsText(g.sets);
          const edited = g.sets.some((s) => s.edited);
          return (
            <li className="exercise" key={`${g.name ?? 'unnamed'}-${i}`}>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                <span className="exercise__name">
                  {g.name ?? <Unavailable />} {edited ? <span className="edited-tag">Edited</span> : null}
                </span>
                <span className="exercise__sub">
                  {g.sets.length} {g.sets.length === 1 ? 'set' : 'sets'} {'·'} {reps ?? <Unavailable />}
                </span>
              </div>
              <span style={{ fontSize: 15, color: load === null ? 'var(--text-2)' : undefined }}>{load ?? 'Bodyweight'}</span>
            </li>
          );
        })}
      </ul>
    </Card>
  );
}

function SetRow({
  set,
  n,
  groupName,
  draft,
  errors,
  onChange,
  onReset,
}: {
  set: StrengthSet;
  n: number;
  groupName: string;
  draft: SetDraft;
  errors: SetErrors | undefined;
  onChange: (patch: Partial<SetDraft>) => void;
  onReset: () => void;
}) {
  const w = watchDraftOf(set);
  const current = draftOf(set);
  const differs = differsFromWatch(set, draft);
  const label = (what: string) => `${groupName}, set ${n}, ${what}`;
  const repsId = `set-${set.idx}-reps-err`;
  const weightId = `set-${set.idx}-weight-err`;
  return (
    <div className="sets__row">
      <span className="sets__num">{n}</span>
      <div className="sets__cell">
        <input
          className="sets__input"
          inputMode="numeric"
          autoComplete="off"
          aria-label={label('reps')}
          aria-invalid={errors?.reps ? true : undefined}
          aria-describedby={errors?.reps ? repsId : undefined}
          data-changed={draft.reps.trim() !== current.reps ? 'true' : undefined}
          data-set={`${set.idx}-reps`}
          value={draft.reps}
          placeholder={w.reps === '' ? 'unknown' : undefined}
          onChange={(e) => onChange({ reps: e.target.value })}
        />
        {errors?.reps ? (
          <span className="sets__error" id={repsId}>
            {errors.reps}
          </span>
        ) : null}
      </div>
      <div className="sets__cell">
        <input
          className="sets__input"
          inputMode="decimal"
          autoComplete="off"
          aria-label={label('weight in kilograms')}
          aria-invalid={errors?.weight ? true : undefined}
          aria-describedby={errors?.weight ? weightId : undefined}
          data-changed={draft.weight.trim().replace(',', '.') !== current.weight ? 'true' : undefined}
          data-set={`${set.idx}-weight`}
          value={draft.weight}
          placeholder="none"
          onChange={(e) => onChange({ weight: e.target.value })}
        />
        {errors?.weight ? (
          <span className="sets__error" id={weightId}>
            {errors.weight}
          </span>
        ) : null}
      </div>
      {differs ? (
        <button type="button" className="sets__reset" aria-label={`Reset set ${n} of ${groupName} to the watch’s value`} title="Reset to the watch’s value" onClick={onReset}>
          <UndoIcon />
        </button>
      ) : (
        <span />
      )}
    </div>
  );
}

/** The exercise list as inputs: reps, weight and the exercise name, with a reset per set. */
function ExerciseEditor({
  groups,
  drafts,
  errors,
  setDrafts,
}: {
  groups: readonly ExerciseGroup[];
  drafts: Drafts;
  errors: Record<number, SetErrors>;
  setDrafts: (updater: (d: Drafts) => Drafts) => void;
}) {
  return (
    <Card padding="list">
      <h2 className="sr-only">Edit sets</h2>
      <p className="acts-note" style={{ paddingTop: 8 }}>
        Leave weight empty for bodyweight and reps empty when unknown. The undo button puts a set back to what the watch recorded.
      </p>
      {groups.map((g, gi) => {
        const names = [...new Set(g.sets.map((s) => (drafts[s.idx] ?? draftOf(s)).exercise))];
        const groupName = names.length === 1 ? (names[0] as string) : '';
        const label = g.name ?? 'Unnamed exercise';
        const nameError = g.sets.map((s) => errors[s.idx]?.exercise).find(Boolean);
        const nameErrId = `group-${gi}-name-err`;
        const nameDiffers = g.sets.some((s) => (drafts[s.idx]?.exercise.trim() ?? '') !== (s.original_exercise ?? ''));
        return (
          <section className="ex-group" key={`${g.sets[0]?.idx ?? gi}`} aria-label={label}>
            <div className="ex-group__head">
              <div className="field">
                <label className="field__label" htmlFor={`group-${gi}-name`}>
                  Exercise
                </label>
                <input
                  id={`group-${gi}-name`}
                  className="field__input"
                  autoComplete="off"
                  value={groupName}
                  placeholder={names.length > 1 ? 'Several names' : 'Exercise name'}
                  aria-invalid={nameError ? true : undefined}
                  aria-describedby={nameError ? nameErrId : undefined}
                  onChange={(e) => {
                    const v = e.target.value;
                    setDrafts((d) => {
                      const next = { ...d };
                      for (const s of g.sets) next[s.idx] = { ...(next[s.idx] ?? draftOf(s)), exercise: v };
                      return next;
                    });
                  }}
                />
                {nameError ? (
                  <span className="sets__error" id={nameErrId}>
                    {nameError}
                  </span>
                ) : null}
              </div>
              {nameDiffers ? (
                <button
                  type="button"
                  className="sets__reset"
                  aria-label={`Reset the name of ${label} to the watch’s`}
                  title="Reset the name to the watch’s"
                  onClick={() =>
                    setDrafts((d) => {
                      const next = { ...d };
                      for (const s of g.sets) next[s.idx] = { ...(next[s.idx] ?? draftOf(s)), exercise: s.original_exercise ?? '' };
                      return next;
                    })
                  }
                >
                  <UndoIcon />
                </button>
              ) : null}
            </div>
            <div className="sets">
              <div className="sets__head" aria-hidden="true">
                <span>Set</span>
                <span>Reps</span>
                <span>Weight (kg)</span>
                <span />
              </div>
              {g.sets.map((s, i) => (
                <SetRow
                  key={s.idx}
                  set={s}
                  n={i + 1}
                  groupName={label}
                  draft={drafts[s.idx] ?? draftOf(s)}
                  errors={errors[s.idx]}
                  onChange={(patch) => setDrafts((d) => ({ ...d, [s.idx]: { ...(d[s.idx] ?? draftOf(s)), ...patch } }))}
                  onReset={() => setDrafts((d) => ({ ...d, [s.idx]: watchDraftOf(s) }))}
                />
              ))}
            </div>
          </section>
        );
      })}
    </Card>
  );
}

type Notice = { kind: 'ok'; text: string } | { kind: 'error'; text: string } | null;

/** The strength view: totals, heart rate, the exercises (view or edit), the comparison and the footer. */
export function StrengthSession({ data }: { data: StrengthResponse }) {
  const desktop = useIsDesktop();
  const save = useSaveStrength(data.id);
  const [editing, setEditing] = useState(false);
  const [comparing, setComparing] = useState(false);
  const [tab, setTab] = useState<'sets' | 'compare'>('sets');
  const [drafts, setDraftsState] = useState<Drafts>({});
  const [attempted, setAttempted] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  const summary = useRef<HTMLDivElement>(null);

  const errors = useMemo(() => (editing ? validateAll(data.exercises, drafts) : {}), [editing, data.exercises, drafts]);
  const errorCount = Object.keys(errors).length;
  const patch = useMemo(() => (editing && errorCount === 0 ? buildPatch(data.exercises, drafts) : []), [editing, errorCount, data.exercises, drafts]);
  const dirty = editing && patch.length > 0;

  // Leaving the page with unsaved edits asks first.
  useEffect(() => {
    if (!dirty) return;
    const h = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener('beforeunload', h);
    return () => window.removeEventListener('beforeunload', h);
  }, [dirty]);

  const startEdit = () => {
    setDraftsState(draftsOf(data.exercises));
    setAttempted(false);
    setNotice(null);
    save.reset();
    setTab('sets');
    setEditing(true);
  };
  const cancelEdit = () => {
    setEditing(false);
    setAttempted(false);
    setNotice(null);
    save.reset();
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (save.isPending) return;
    if (errorCount > 0) {
      setAttempted(true);
      summary.current?.focus();
      return;
    }
    if (patch.length === 0) {
      setNotice({ kind: 'ok', text: 'No changes to save.' });
      return;
    }
    save.mutate(
      { sets: patch },
      {
        onSuccess: () => {
          setEditing(false);
          setNotice({ kind: 'ok', text: `Saved. ${patch.length} ${patch.length === 1 ? 'set' : 'sets'} changed.` });
        },
        onError: (err) => setNotice({ kind: 'error', text: `Not saved. ${errorText(err)} Your changes are still here.` }),
      },
    );
  };

  const hr = timedToMinutes(data.heart_rate);
  const hrSpread = spread(hr.y);
  const hrAside = data.avg_hr_bpm === null && data.max_hr_bpm === null ? null : `Avg ${fmtInt(data.avg_hr_bpm) ?? 'unavailable'} · Max ${fmtInt(data.max_hr_bpm) ?? 'unavailable'} bpm`;
  const hrSummary = hrSpread
    ? `Heart rate during the session: lowest ${Math.round(hrSpread.min)}, average ${Math.round(hrSpread.avg)}, highest ${Math.round(hrSpread.max)} beats per minute.`
    : 'No heart rate samples were recorded.';

  const exercises = editing ? (
    <form id={FORM_ID} onSubmit={onSubmit} noValidate>
      <ExerciseEditor groups={data.exercises} drafts={drafts} errors={errors} setDrafts={(u) => setDraftsState(u)} />
    </form>
  ) : data.exercises.length === 0 ? (
    <Card>
      <CardHeader title="Exercises" tone="muted" />
      <p>
        <Unavailable />
      </p>
      <p className="acts-note">The watch did not record any sets for this session.</p>
    </Card>
  ) : (
    <ExerciseList groups={data.exercises} />
  );

  const compare = comparing && !editing ? <StrengthCompare current={data} /> : null;

  const messages = (
    <>
      {notice ? (
        <p className={`status-msg status-msg--${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'}>
          {notice.text}
        </p>
      ) : null}
      {attempted && errorCount > 0 ? (
        <div className="status-msg status-msg--error" role="alert" tabIndex={-1} ref={summary}>
          Fix {errorCount} {errorCount === 1 ? 'set' : 'sets'} before saving:
          <ul>
            {Object.entries(errors).map(([idx, e]) => (
              <li key={idx}>{[e.reps, e.weight, e.exercise].filter(Boolean).join(' ')}</li>
            ))}
          </ul>
        </div>
      ) : null}
    </>
  );

  if (desktop) {
    const prev = data.previous;
    const day = data.start.slice(0, 10);
    const when = { date: day, start: data.start };
    const editFooter = editing ? (
      <div className="footer-bar">
        <Button size="lg" onClick={cancelEdit} disabled={save.isPending}>
          Discard changes
        </Button>
        <Button size="lg" variant="primary" type="submit" form={FORM_ID} disabled={save.isPending || (!dirty && errorCount === 0)} aria-busy={save.isPending}>
          {save.isPending ? 'Saving' : 'Save changes'}
        </Button>
      </div>
    ) : tab === 'sets' ? (
      <div className="footer-bar footer-bar--single">
        <Button size="lg" onClick={startEdit} disabled={data.exercises.length === 0}>
          Edit sets
        </Button>
      </div>
    ) : null;
    const main = (
      <>
        <DetailHeader
          backTo="/strength"
          section="Strength"
          noun="Strength training"
          when={longWhen(when)}
          kind="strength"
          title="Strength"
          sample={data.sample}
          headline={[
            { label: 'Time', value: fmtClock(data.duration_s) },
            { label: 'Sets', value: data.exercises.length === 0 && data.sets === 0 ? null : String(data.sets) },
            { label: 'Volume', value: fmtKg(data.volume_kg === null ? null : Math.round(data.volume_kg)) },
            { label: 'Avg HR', value: fmtBpm(data.avg_hr_bpm) },
            { label: 'Max HR', value: fmtBpm(data.max_hr_bpm) },
          ]}
          prev={prev ? { to: `/activities/${prev.id}/strength`, label: `Previous session, ${fmtShortDay(prev.start.slice(0, 10))}` } : null}
        />
        <ChartBlock
          color="heart"
          title="Heart rate"
          label="Heart rate during the session"
          summary={hrSummary}
          x={hr.x}
          data={hr.y}
          xLabels={hrSpread ? timeLabels(hr.x[hr.x.length - 1] ?? 0) : []}
          {...(hrSpread && data.avg_hr_bpm !== null ? { refLine: { value: data.avg_hr_bpm, label: `Avg: ${Math.round(data.avg_hr_bpm)} bpm` } } : {})}
        />
        {messages}
        <Tabs
          label="Session details"
          value={editing ? 'sets' : tab}
          onChange={(id) => setTab(id === 'compare' ? 'compare' : 'sets')}
          tabs={[
            { id: 'sets', label: 'Sets', content: exercises },
            { id: 'compare', label: 'Compare to last', disabled: editing, content: <StrengthCompare current={data} /> },
          ]}
        />
        {editFooter}
      </>
    );
    const side = prev ? (
      <SideCard title="Previous session">
        <SideFacts
          items={[
            { label: 'Date', value: fmtShortDay(prev.start.slice(0, 10)) },
            { label: 'Time', value: fmtMinutes(prev.duration_s) },
            { label: 'Sets', value: prev.sets === null ? null : String(prev.sets) },
            { label: 'Volume', value: fmtKg(prev.volume_kg === null ? null : Math.round(prev.volume_kg)) },
          ]}
        />
        <Link to={`/activities/${prev.id}/strength`}>Open previous session</Link>
      </SideCard>
    ) : (
      <SideCard title="Previous session">
        <p className="adt-note">There is no earlier strength session to compare with.</p>
      </SideCard>
    );
    return (
      <div className="adt-root" data-activity-id={data.id}>
        <DetailLayout main={main} side={side} />
      </div>
    );
  }

  return (
    <>
      <div className="page" data-activity-id={data.id}>
        <div className={desktop ? 'grid-tiles' : 'grid-3'}>
          <StatTile size={desktop ? 'md' : 'sm'} label="Duration" value={fmtMinutes(data.duration_s)} />
          <StatTile size={desktop ? 'md' : 'sm'} label="Sets" value={data.exercises.length === 0 && data.sets === 0 ? null : String(data.sets)} />
          <StatTile size={desktop ? 'md' : 'sm'} label="Volume" value={fmtKg(data.volume_kg === null ? null : Math.round(data.volume_kg))} />
        </div>
        <Card>
          <CardHeader title="Heart rate" tone="muted" aside={hrAside} />
          <SeriesChart
            label="Heart rate during the session"
            summary={hrSummary}
            x={hr.x}
            series={[{ label: 'Heart rate', color: 'heart', data: hr.y }]}
            height={desktop ? 120 : 96}
            xLabels={hrSpread ? timeLabels(hr.x[hr.x.length - 1] ?? 0) : []}
          />
        </Card>

        {messages}

        {desktop && compare ? (
          <div className="strength-grid">
            {exercises}
            {compare}
          </div>
        ) : (
          <>
            {exercises}
            {compare}
          </>
        )}
      </div>

      <div className="footer-bar">
        {editing ? (
          <>
            <Button size="lg" onClick={cancelEdit} disabled={save.isPending}>
              Discard changes
            </Button>
            <Button size="lg" variant="primary" type="submit" form={FORM_ID} disabled={save.isPending || (!dirty && errorCount === 0)} aria-busy={save.isPending}>
              {save.isPending ? 'Saving' : 'Save changes'}
            </Button>
          </>
        ) : (
          <>
            <Button size="lg" onClick={startEdit} disabled={data.exercises.length === 0}>
              Edit sets
            </Button>
            <Button size="lg" variant="primary" aria-pressed={comparing} onClick={() => setComparing((c) => !c)}>
              {comparing ? 'Hide comparison' : 'Compare to last'}
            </Button>
          </>
        )}
      </div>
    </>
  );
}
