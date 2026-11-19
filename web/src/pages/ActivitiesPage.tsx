import { useSearchParams } from 'react-router-dom';
import { EmptyState, FilterChips, ListRow, SampleDataChip, TopBar } from '../components';
import { usePageTitle } from '../lib/hooks';

// STUB (stage 3d wires it to GET /api/activities): static canvas values.

const TYPES = [
  { value: 'all', label: 'All' },
  { value: 'run', label: 'Run' },
  { value: 'strength', label: 'Strength' },
  { value: 'walk', label: 'Walk' },
] as const;
type TypeKey = (typeof TYPES)[number]['value'];

interface Row {
  id: string;
  kind: Exclude<TypeKey, 'all'>;
  title: string;
  when: string;
  value: string;
  sub: string;
  group: 'This week' | 'Last week';
}

const ROWS: Row[] = [
  { id: '1', kind: 'strength', title: 'Strength', when: 'Wed 18 Nov · 18:05', value: '42 min', sub: '18 sets · 3,300 kg', group: 'This week' },
  { id: '2', kind: 'run', title: 'Run', when: 'Mon 16 Nov · 17:40', value: '6.4 km', sub: '38:00 · 5:56 /km', group: 'This week' },
  { id: '3', kind: 'strength', title: 'Strength', when: 'Sun 15 Nov · 10:20', value: '45 min', sub: '18 sets · 3,180 kg', group: 'Last week' },
  { id: '4', kind: 'walk', title: 'Walk', when: 'Fri 13 Nov · 12:10', value: '4.8 km', sub: '52 min', group: 'Last week' },
  { id: '5', kind: 'run', title: 'Run', when: 'Wed 11 Nov · 17:35', value: '5.1 km', sub: '30:40 · 6:01 /km', group: 'Last week' },
  { id: '6', kind: 'strength', title: 'Strength', when: 'Tue 10 Nov · 18:15', value: '40 min', sub: '17 sets · 3,050 kg', group: 'Last week' },
];

export function ActivitiesPage() {
  usePageTitle('Activities');
  const [params, setParams] = useSearchParams();
  const raw = params.get('type');
  const type: TypeKey = TYPES.some((t) => t.value === raw) ? (raw as TypeKey) : 'all';
  const rows = ROWS.filter((r) => type === 'all' || r.kind === type);
  const groups = (['This week', 'Last week'] as const).filter((g) => rows.some((r) => r.group === g));

  return (
    <>
      <TopBar variant="large" title="Activities" aside={<SampleDataChip sample />} />
      <FilterChips
        label="Filter by type"
        options={TYPES}
        value={type}
        onChange={(v) => setParams(v === 'all' ? {} : { type: v }, { replace: true })}
      />
      <div className="page" style={{ paddingTop: 0 }}>
        {rows.length === 0 ? (
          <EmptyState title="No activities of this type">Pick another type, or sync the watch to bring in new activities.</EmptyState>
        ) : (
          groups.map((g) => (
            <section key={g} aria-label={g} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <h2 className="section-label">{g}</h2>
              {rows
                .filter((r) => r.group === g)
                .map((r) => (
                  <ListRow key={r.id} to={r.kind === 'strength' ? `/activities/${r.id}/strength` : `/activities/${r.id}`} title={r.title} sub={r.when} value={r.value} valueSub={r.sub} />
                ))}
            </section>
          ))
        )}
      </div>
    </>
  );
}
