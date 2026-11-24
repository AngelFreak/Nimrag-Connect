import { useId, useState } from 'react';
import type { ExplainFactor } from '../../api/generated/ExplainFactor';
import type { FocusRow } from '../../api/generated/FocusRow';
import type { StatusExplain } from '../../api/generated/StatusExplain';
import { Skeleton } from '../../components';
import { FactorList, FactorRow } from './desktop';
import { fmtNum } from './readiness';
import { useStatusExplain } from './readinessQueries';

// "Why this status": one sentence from Nimrag's own rules, what the status means, and (on the
// Training status page) the figures behind it. The watch decides the status; this only explains it.

const FACTOR_TO: Record<string, string> = {
  vo2max: '/metrics/vo2_max',
  load: '/metrics/training_load',
  hrv: '/hrv',
};

const FOCUS_WORD: Record<FocusRow['state'], string> = {
  short: 'Short of target',
  in_range: 'In range',
  above: 'Above range',
  unknown: 'Not available',
};

function signed(v: number): string {
  const n = fmtNum(v) ?? '';
  return v > 0 ? `+${n}` : n;
}

function factorSub(f: ExplainFactor): string {
  const value = fmtNum(f.value);
  if (value === null) return 'Not available yet';
  const parts = [`${value} ${f.unit === 'load' ? '' : f.unit}`.trim()];
  if (f.change !== null) parts.push(`${signed(f.change)} over 4 weeks`);
  if (f.detail) parts.push(f.detail);
  return parts.join(' · ');
}

function focusSub(r: FocusRow): string {
  const value = fmtNum(r.value);
  if (value === null) return 'Not available yet';
  const range = r.target_min !== null && r.target_max !== null ? `Target ${fmtNum(r.target_min)}–${fmtNum(r.target_max)}` : null;
  return [value, range].filter(Boolean).join(' · ');
}

/** The paragraph "What this status means", closed until asked for. */
function Meaning({ meaning }: { meaning: NonNullable<StatusExplain['meaning']> }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <div className="explain__more">
      <button type="button" aria-expanded={open} aria-controls={id} onClick={() => setOpen((v) => !v)}>
        What this status means
      </button>
      <p className="explain__meaning" id={id} hidden={!open}>
        {meaning.text}
      </p>
    </div>
  );
}

function Figures({ data }: { data: StatusExplain }) {
  const shortLabel = data.focus.find((r) => r.key === data.short_of)?.label ?? null;
  return (
    <>
      <FactorList tight label="Figures behind the status">
        {data.factors.map((f) => (
          <FactorRow key={f.key} to={FACTOR_TO[f.key] ?? '/training'} name={f.label} sub={factorSub(f)} rating={f.word ?? <span className="rate rate--none">Not available</span>} />
        ))}
      </FactorList>
      {data.focus.length > 0 ? (
        <>
          <p className="explain__sentence" style={{ marginTop: 4 }}>
            <strong>Load focus.</strong> {shortLabel ? `${shortLabel} is furthest short of its target.` : 'None of the three intensity buckets is short of its target.'}
          </p>
          <FactorList tight label="Load focus by intensity">
            {data.focus.map((r) => (
              <FactorRow key={r.key} to="/training" name={r.label} sub={focusSub(r)} rating={FOCUS_WORD[r.state]} />
            ))}
          </FactorList>
        </>
      ) : null}
    </>
  );
}

/**
 * `brief`: the sentence and the meaning (the Training page, which shows the figures itself).
 * Otherwise also the figures behind the status and the load focus.
 */
export function StatusExplanation({ brief }: { brief?: boolean }) {
  const q = useStatusExplain();
  if (q.isPending) {
    return (
      <div className="explain" aria-busy="true">
        <Skeleton height={16} lines={2} />
      </div>
    );
  }
  if (q.isError || !q.data) return <p className="stat__sub">The explanation could not be loaded.</p>;
  const data = q.data;
  return (
    <div className="explain">
      {data.sentence ? <p className="explain__sentence">{data.sentence}</p> : <p className="stat__sub">No training status has been recorded yet, so there is nothing to explain.</p>}
      {data.meaning ? <Meaning meaning={data.meaning} /> : null}
      {brief ? null : <Figures data={data} />}
    </div>
  );
}
