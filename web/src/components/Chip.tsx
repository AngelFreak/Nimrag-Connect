import type { ReactNode } from 'react';
import { useGroupArrows, type Option } from './SegmentedControl';

/** Small outlined pill for status text. */
export function Chip({ children }: { children: ReactNode }) {
  return <span className="chip">{children}</span>;
}

/**
 * "Sample data" marker. Shown only when the API says the response is seeded sample data
 * (`sample: true`); with anything else it renders nothing.
 */
export function SampleDataChip({ sample }: { sample: boolean | null | undefined }) {
  return sample === true ? <Chip>Sample data</Chip> : null;
}

interface FilterChipsProps<V extends string> {
  /** Accessible name of the group ("Filter by type"). */
  label: string;
  options: readonly Option<V>[];
  value: V;
  onChange: (value: V) => void;
}

/** Single-choice filter pills (Activities: All, Run, Strength, Walk). */
export function FilterChips<V extends string>({ label, options, value, onChange }: FilterChipsProps<V>) {
  const ref = useGroupArrows<HTMLDivElement>();
  return (
    <div ref={ref} role="group" aria-label={label} className="filter-chips" data-scroll="x">
      {options.map((o) => (
        <button key={o.value} type="button" className="filter-chip" aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}
