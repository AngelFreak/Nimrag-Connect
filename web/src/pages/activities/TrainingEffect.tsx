import type { ActivityDetail } from '../../api/generated';
import { fmtFixed } from '../../lib/format';
import { TE_MAX, teFill, teRows, type TeRow } from './trainingEffect';
import './activities.css';

type Source = Pick<ActivityDetail, 'aerobic_te' | 'anaerobic_te' | 'training_load'>;

/** True when there is anything to show: an effect or the load. */
export function hasTrainingEffect(a: Source): boolean {
  return teRows(a).length > 0 || fmtFixed(a.training_load, 0) !== null;
}

function Bar({ row }: { row: TeRow }) {
  return (
    <div className="te__bar" role="meter" aria-label={`${row.label} training effect`} aria-valuemin={0} aria-valuemax={TE_MAX} aria-valuenow={Math.min(TE_MAX, Math.max(0, row.value))} aria-valuetext={`${row.text} of ${TE_MAX}, ${row.word}`}>
      {teFill(row.value).map((fill, i) => (
        <span key={i} className="te__step">
          <span className={`te__fill te__fill--${row.key}`} style={{ width: `${fill * 100}%` }} />
        </span>
      ))}
    </div>
  );
}

/** Aerobic and anaerobic effect as a value, a five-step bar and a word, with the activity's training load below. */
export function TrainingEffect({ a }: { a: Source }) {
  const rows = teRows(a);
  const load = fmtFixed(a.training_load, 0);
  return (
    <div className="te">
      {rows.map((r) => (
        <div key={r.key} className="te__row">
          <div className="te__top">
            <span className="te__label">{r.label}</span>
            <span className="te__word">{r.word}</span>
            <span className="te__value">{r.text}</span>
          </div>
          <Bar row={r} />
        </div>
      ))}
      {load !== null ? (
        <div className="te__load">
          <span>Training load</span>
          <span className="te__load-value">{load}</span>
        </div>
      ) : null}
    </div>
  );
}
