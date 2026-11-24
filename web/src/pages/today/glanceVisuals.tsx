import type { ReactNode } from 'react';
import type { Today } from '../../api/generated';
import { RangeBar, Ring } from '../../components';
import { addDays } from '../../lib/dates';
import { isMissing } from '../../lib/format';
import { stressLevelOf, LEVEL_COLOR } from '../../lib/stress';
import { STATUS_COLOR } from '../health/ranges';
import { useTrainingStatus } from '../health/queries';
import { useMetricRange } from '../metrics/queries';
import { Sparkline } from './homeCharts';

// The small picture under the number of an "At a glance" card on a phone. Each is decorative: the
// numbers above it say everything, so the box is hidden from screen readers. A card without the data
// for its picture shows no picture (the functions return `null`).

/** Height of every picture box, so cards do not move when a picture arrives. */
export const VISUAL_HEIGHT = 44;

function Box({ kind, children, loading = false }: { kind: string; children?: ReactNode; loading?: boolean }) {
  return (
    <div className={loading ? 'gv gv--loading' : 'gv'} aria-hidden="true" data-visual={kind}>
      {children}
    </div>
  );
}

const known = (values: (number | null)[]): boolean => values.some((v) => v !== null && Number.isFinite(v));

/** Today's heart rate as a line, the resting rate as a faint dashed line across it. */
export function HeartRateVisual({ today }: { today: Today }) {
  const values = today.heart_rate.day.series.map((p) => p.value);
  if (!known(values)) return null;
  const resting = today.heart_rate.resting_hr_bpm;
  return (
    <Box kind="heart_rate">
      <Sparkline values={values} color="var(--heart)" height={VISUAL_HEIGHT} dot={false} label="Heart rate today" {...(isMissing(resting) ? {} : { reference: resting })} />
    </Box>
  );
}

/** The day's average stress as a ring, in the colour of its level. */
export function StressVisual({ today }: { today: Today }) {
  const avg = today.stress.average;
  if (isMissing(avg)) return null;
  return (
    <Box kind="stress">
      <Ring value={avg} label="Average stress" color={LEVEL_COLOR[stressLevelOf(avg)]} size={VISUAL_HEIGHT} strokeWidth={6} showValue={false} />
    </Box>
  );
}

/** This week's intensity minutes against the weekly goal. */
export function IntensityVisual({ today }: { today: Today }) {
  const m = today.intensity_minutes;
  if (isMissing(m.total_min) || m.goal_min <= 0) return null;
  return (
    <Box kind="intensity_minutes">
      <Ring value={m.total_min} max={m.goal_min} label="Intensity minutes" color="var(--energy)" size={VISUAL_HEIGHT} strokeWidth={6} showValue={false} />
    </Box>
  );
}

/** The 7-day load against its target range (the bar of the Training page). */
export function LoadVisual({ today }: { today: Today }) {
  const l = today.training_load;
  if (isMissing(l.seven_day_load) || isMissing(l.target_min) || isMissing(l.target_max)) return null;
  const max = Math.max(l.target_max, l.seven_day_load, 1) * 1.4;
  return (
    <Box kind="training_load">
      <div className="gv__center">
        <RangeBar value={l.seven_day_load} min={0} max={max} target={[l.target_min, l.target_max]} label="7-day load against its target range" />
      </div>
    </Box>
  );
}

/** Recovery time left as a ring that empties as the time runs down. */
export function RecoveryVisual({ today }: { today: Today }) {
  const r = today.recovery_time;
  if (isMissing(r.remaining_s)) return null;
  let left: number | null = null;
  if (!isMissing(r.total_s) && r.total_s > 0) left = (r.remaining_s / r.total_s) * 100;
  else if (!isMissing(r.percent_complete)) left = 100 - r.percent_complete;
  if (left === null) return null;
  return (
    <Box kind="recovery_time">
      <Ring value={Math.min(100, Math.max(0, left))} label="Recovery time left" color="var(--zone-2)" size={VISUAL_HEIGHT} strokeWidth={6} showValue={false} />
    </Box>
  );
}

/** VO2 max over the last three months. */
export function Vo2Visual({ today }: { today: Today }) {
  const values = today.vo2max.series_3m.map((d) => d.value);
  if (!known(values)) return null;
  return (
    <Box kind="vo2_max">
      <Sparkline values={values} color="var(--zone-4)" height={VISUAL_HEIGHT} label="VO2 max, last 3 months" />
    </Box>
  );
}

/** The last 7 days of steps as seven small bars; those that reached the goal in full colour. */
export function StepsWeekVisual({ today }: { today: Today }) {
  const q = useMetricRange('steps', '7d', today.date);
  if (q.isPending) return <Box kind="steps_week" loading />;
  const values = q.data?.series.find((s) => s.key === 'steps')?.points.map((p) => p.value) ?? [];
  if (!known(values)) return null;
  const goal = today.steps.goal > 0 ? today.steps.goal : null;
  const top = Math.max(1, goal ?? 0, ...values.map((v) => v ?? 0));
  return (
    <Box kind="steps_week">
      <div className="gv__bars">
        {values.map((v, i) => {
          const has = v !== null && v > 0;
          const reached = has && goal !== null && v >= goal;
          return (
            <span className="gv__slot" key={i}>
              <span
                className={has ? 'gv__bar' : 'gv__bar gv__bar--empty'}
                style={has ? { height: `${Math.max(8, (v / top) * 100)}%`, background: goal === null || reached ? 'var(--steps)' : 'var(--steps-dim)' } : undefined}
              />
            </span>
          );
        })}
      </div>
    </Box>
  );
}

/** Runs of equal status in a list of days: [kind, length]. A day without a status is `none`. */
export function statusRuns(days: { status: { kind: string } | null }[]): { kind: string; length: number }[] {
  const runs: { kind: string; length: number }[] = [];
  for (const d of days) {
    const kind = d.status?.kind ?? 'none';
    const last = runs[runs.length - 1];
    if (last && last.kind === kind) last.length += 1;
    else runs.push({ kind, length: 1 });
  }
  return runs;
}

/** The training status of the last 28 days: a run of the same status is one block, so it reads as periods. */
export function StatusStripVisual({ today }: { today: Today }) {
  const from = addDays(today.date, -27);
  const q = useTrainingStatus(from, today.date);
  if (q.isPending) return <Box kind="training_status" loading />;
  const days = q.data?.days ?? [];
  if (!days.some((d) => d.status !== null)) return null;
  return (
    <Box kind="training_status">
      <div className="gv__strip">
        {statusRuns(days).map((r, i) => (
          <span
            key={i}
            className="gv__block"
            data-kind={r.kind}
            data-days={r.length}
            style={{ flexGrow: r.length, background: r.kind === 'none' ? 'var(--track)' : STATUS_COLOR[r.kind as keyof typeof STATUS_COLOR] }}
          />
        ))}
      </div>
    </Box>
  );
}
