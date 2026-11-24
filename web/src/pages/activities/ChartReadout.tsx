import type { ReactNode } from 'react';
import type { ActivitySeries } from '../../api/generated/ActivitySeries';
import { useCursorIndex, type CursorStore } from '../../components';
import { sampleAt, type SampleText } from './series';
import './activities.css';

export type ReadoutField = 'pace' | 'speed' | 'hr' | 'elevation' | 'cadence';

const FIELD_LABEL: Record<ReadoutField, string> = {
  pace: 'Pace',
  speed: 'Speed',
  hr: 'Heart rate',
  elevation: 'Elevation',
  cadence: 'Cadence',
};

/**
 * The values of every chart at the point the pointer is on, in one line above the charts (wide
 * screen). The line keeps its height when the pointer is away, so nothing moves.
 */
export function ChartReadout({ store, series, fields, cadenceUnit }: { store: CursorStore; series: ActivitySeries | null; fields: readonly ReadoutField[]; cadenceUnit?: string }) {
  const index = useCursorIndex(store);
  const at = sampleAt(series, index, cadenceUnit);
  // The readout repeats what the charts and their summaries say; it is an aid for the pointer.
  return (
    <p className="chart-readout" aria-hidden="true">
      {at ? (
        <>
          <span className="chart-readout__where">{[at.time, at.distance].filter(Boolean).join(' · ')}</span>
          {fields.map((f) => (
            <span key={f} className="chart-readout__item">
              <span className="chart-readout__label">{FIELD_LABEL[f]}</span> {at[f] ?? '–'}
            </span>
          ))}
        </>
      ) : (
        <span className="chart-readout__hint">Move over a chart to read the values at that point.</span>
      )}
    </p>
  );
}

/** The right-hand text of a chart card on a phone: the value where the finger is, or `fallback` (the average) when nowhere. */
export function ReadoutAside({ store, series, field, fallback }: { store: CursorStore; series: ActivitySeries | null; field: ReadoutField; fallback: ReactNode }) {
  const index = useCursorIndex(store);
  const at: SampleText | null = sampleAt(series, index);
  if (!at) return <>{fallback}</>;
  return (
    <>
      {at.time} · {at[field] ?? '–'}
    </>
  );
}
