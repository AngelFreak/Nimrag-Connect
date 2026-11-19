import type { ReactNode } from 'react';
import type { StrengthResponse } from '../../api/generated';
import { Card, CardHeader, EmptyState, ErrorState, Skeleton, StatTile, Table, Unavailable } from '../../components';
import { fmtShortDay } from '../../lib/dates';
import { fmtInt } from '../../lib/format';
import { useStrength } from './api';
import { fmtKg, fmtMinutes } from './format';
import { errorText } from './PageStates';
import { compareExercises, deltaText, type ExerciseComparison, type ExerciseTotals } from './strengthModel';

function Pair({ now, last }: { now: ReactNode; last: ReactNode }) {
  return (
    <span style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <span>{now}</span>
      <span className="acts-note">{last}</span>
    </span>
  );
}

function lastText(before: number | null, text: string | null, delta: string | null): string {
  if (before === null || text === null) return 'last: unavailable';
  if (delta === 'same') return 'same';
  return `was ${text}${delta ? ` (${delta})` : ''}`;
}

function totalSub(last: string | null, delta: string | null): string {
  return `Last: ${last ?? 'unavailable'}${last !== null && delta ? ` (${delta})` : ''}`;
}

function setsCell(r: ExerciseComparison): ReactNode {
  return (
    <Pair
      now={r.now.sets}
      last={r.before ? lastText(r.before.sets, String(r.before.sets), deltaText(r.now.sets, r.before.sets)) : 'not in the last session'}
    />
  );
}

function weightCell(r: ExerciseComparison): ReactNode {
  const b = r.before?.topWeight ?? null;
  if (r.now.topWeight === null && b === null) return <Pair now="Bodyweight" last="" />;
  return (
    <Pair
      now={fmtKg(r.now.topWeight) ?? <Unavailable />}
      last={r.before ? lastText(b, fmtKg(b), deltaText(r.now.topWeight, b, ' kg', 1)) : ''}
    />
  );
}

function volumeCell(r: ExerciseComparison): ReactNode {
  const b = r.before?.volume ?? null;
  const now = r.now.volume === null ? null : Math.round(r.now.volume);
  if (now === null && b === null) return <Pair now="No load" last="" />;
  return (
    <Pair
      now={fmtKg(now) ?? <Unavailable />}
      last={r.before ? lastText(b, fmtKg(b === null ? null : Math.round(b)), deltaText(now, b === null ? null : Math.round(b), ' kg')) : ''}
    />
  );
}

/** This session against the strength session before it: totals from the list, exercises from that session's own sets. */
export function StrengthCompare({ current }: { current: StrengthResponse }) {
  const prev = current.previous;
  const before = useStrength(prev?.id, !!prev);

  if (!prev) {
    return (
      <Card>
        <EmptyState title="No earlier strength session">There is no earlier session to compare this one with yet.</EmptyState>
      </Card>
    );
  }

  const when = fmtShortDay(prev.start.slice(0, 10));
  const nowVolume = current.volume_kg === null ? null : Math.round(current.volume_kg);
  const prevVolume = prev.volume_kg === null ? null : Math.round(prev.volume_kg);
  const nowMin = current.duration_s === null ? null : Math.round(current.duration_s / 60);
  const prevMin = prev.duration_s === null ? null : Math.round(prev.duration_s / 60);
  const cmp = before.data ? compareExercises(current.exercises, before.data.exercises) : null;

  return (
    <Card aria-label={`Compared with ${when}`}>
      <CardHeader title={`Compared with ${when}`} tone="muted" aside={null} />
      <div className="grid-3">
        <StatTile size="sm" label="Duration" value={fmtMinutes(current.duration_s)} sub={totalSub(fmtMinutes(prev.duration_s), deltaText(nowMin, prevMin, ' min'))} />
        <StatTile size="sm" label="Sets" value={String(current.sets)} sub={totalSub(fmtInt(prev.sets), deltaText(current.sets, prev.sets))} />
        <StatTile size="sm" label="Volume" value={fmtKg(nowVolume)} sub={totalSub(fmtKg(prevVolume), deltaText(nowVolume, prevVolume, ' kg'))} />
      </div>

      {before.isPending ? (
        <div aria-busy="true">
          <span className="sr-only">Loading the last session</span>
          <Skeleton height={14} lines={4} />
        </div>
      ) : before.isError ? (
        <ErrorState title="Exercise details of the last session could not be loaded">{errorText(before.error)}</ErrorState>
      ) : cmp ? (
        <>
          <div className="cmp-table">
          <Table
            caption="Exercises, this session against the last"
            density="compact"
            rows={cmp.rows}
            rowKey={(r) => r.name}
            columns={[
              { key: 'ex', header: 'Exercise', cell: (r) => r.name },
              { key: 'sets', header: 'Sets', cell: setsCell },
              { key: 'w', header: 'Top weight', cell: weightCell },
              { key: 'v', header: 'Volume', cell: volumeCell, align: 'end' },
            ]}
          />
          </div>
          {cmp.onlyBefore.length > 0 ? (
            <p className="acts-note">Only in the last session: {cmp.onlyBefore.map((e: ExerciseTotals) => e.name).join(', ')}.</p>
          ) : null}
        </>
      ) : null}
    </Card>
  );
}
