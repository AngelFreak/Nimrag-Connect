import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { api } from '../api/client';
import type { BodyEntry, BodyResponse } from '../api/generated';
import { Button, Card, EmptyState, PageHeader, SampleDataChip, StatStrip, StatTile, TopBar, ValueOrUnavailable } from '../components';
import { browserToday, fmtShortDay } from '../lib/dates';
import { fmtFixed } from '../lib/format';
import { useIsDesktop, usePageTitle } from '../lib/hooks';
import { StatsSection } from './health/desktop';
import { EntryForm } from './body/EntryForm';
import { WeightChart } from './body/WeightChart';
import { entryWhat, fmtChange, fmtEntryWhen } from './body/body';
import { errorText, PageError, PageSkeleton, useOwnerToday } from './body/common';
import { localParts } from './body/time';

// Stage 3k: GET /api/body, POST /api/body (add), DELETE /api/body/{id}. Requests that change
// something carry the X-Nimrag header through the client.

export function BodyPage() {
  usePageTitle('Body');
  const ownerToday = useOwnerToday();
  const query = useQuery({
    queryKey: ['body'],
    queryFn: ({ signal }) => api.get<BodyResponse>('/api/body', { signal }),
  });
  const data = query.data;
  const desktop = useIsDesktop();

  return (
    <>
      {desktop ? (
        <PageHeader title="Weight & Body" actions={<SampleDataChip sample={data?.sample} />} />
      ) : (
        <TopBar variant="back" backTo="/health" title="Body" subtitle="Weight and measurements" aside={<SampleDataChip sample={data?.sample} />} />
      )}
      {query.isError ? (
        <PageError title="Body measurements could not be loaded" error={query.error} onRetry={() => void query.refetch()} />
      ) : !data ? (
        <PageSkeleton cards={3} />
      ) : (
        <BodyBody data={data} today={ownerToday ?? browserToday()} />
      )}
    </>
  );
}

export function BodyBody({ data, today }: { data: BodyResponse; today: string }) {
  const [adding, setAdding] = useState(false);
  const [saved, setSaved] = useState(false);

  const weightDay = localParts(data.latest_weight?.at)?.day;
  const fatDay = localParts(data.latest_body_fat?.at)?.day;
  const waistDay = localParts(data.latest_waist?.at)?.day;
  const change = fmtChange(data.weight_change_30d_kg);
  const open = () => {
    setSaved(false);
    setAdding(true);
  };
  const desktop = useIsDesktop();

  if (desktop) {
    return (
      <div className="page" style={{ paddingTop: 0 }}>
        <div className="stats-stack">
          <StatsSection title="Weight" aside={weightDay ? fmtShortDay(weightDay) : undefined}>
            <WeightChart weights={data.weights} trend={data.weight_trend} />
            <StatStrip
              items={[
                { key: 'weight', label: 'Weight', value: data.latest_weight ? `${fmtFixed(data.latest_weight.value, 1)} kg` : null },
                { key: 'change', label: '30-day change', value: change },
                { key: 'fat', label: fatDay ? `Body fat, ${fmtShortDay(fatDay)}` : 'Body fat', value: data.latest_body_fat ? `${fmtFixed(data.latest_body_fat.value, 1)} %` : null },
                { key: 'waist', label: waistDay ? `Waist, ${fmtShortDay(waistDay)}` : 'Waist', value: data.latest_waist ? `${fmtFixed(data.latest_waist.value, 1)} cm` : null },
              ]}
            />
          </StatsSection>

          {saved ? (
            <p role="status" className="stat__sub tone-energy">
              Entry saved.
            </p>
          ) : null}

          {adding ? (
            <div style={{ maxWidth: 640 }}>
              <EntryForm
                today={today}
                onCancel={() => setAdding(false)}
                onDone={() => {
                  setAdding(false);
                  setSaved(true);
                }}
              />
            </div>
          ) : null}

          <Entries entries={data.entries} onAdd={open} adding={adding} />
        </div>
      </div>
    );
  }

  return (
    <>
      <div className="page">
        <Card padding="lg">
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', gap: 12, flexWrap: 'wrap' }}>
            <div className="stat">
              <h2 className="card__title card__title--muted">Weight{weightDay ? ` · ${fmtShortDay(weightDay)}` : ''}</h2>
              <span className="stat__value" style={{ fontSize: 34 }}>
                <ValueOrUnavailable value={data.latest_weight ? `${fmtFixed(data.latest_weight.value, 1)} kg` : null} />
              </span>
            </div>
            <span className="stat__sub">30-day change: {change ?? <span className="unavailable">unavailable</span>}</span>
          </div>
          <WeightChart weights={data.weights} trend={data.weight_trend} />
        </Card>

        <div className="grid-2">
          <StatTile
            label="Body fat"
            value={data.latest_body_fat ? `${fmtFixed(data.latest_body_fat.value, 1)} %` : null}
            sub={fatDay ? fmtShortDay(fatDay) : 'Manual or scale entry'}
          />
          <StatTile
            label="Waist"
            value={data.latest_waist ? `${fmtFixed(data.latest_waist.value, 1)} cm` : null}
            sub={waistDay ? fmtShortDay(waistDay) : 'Manual entry'}
          />
        </div>

        {saved ? (
          <p role="status" className="stat__sub tone-energy">
            Entry saved.
          </p>
        ) : null}

        {adding ? (
          <div style={{ maxWidth: 640 }}>
            <EntryForm
              today={today}
              onCancel={() => setAdding(false)}
              onDone={() => {
                setAdding(false);
                setSaved(true);
              }}
            />
          </div>
        ) : null}

        <Entries entries={data.entries} onAdd={open} />
      </div>
      {adding ? null : (
        <div className="footer-bar footer-bar--single">
          <Button size="lg" variant="primary" block onClick={open}>
            Add entry
          </Button>
        </div>
      )}
    </>
  );
}

/** How many entries the list shows before "Show all" (the server sends them newest first). */
export const ENTRIES_SHOWN = 10;

function Entries({ entries, onAdd, adding = false }: { entries: BodyEntry[]; onAdd: () => void; adding?: boolean }) {
  const desktop = useIsDesktop();
  const client = useQueryClient();
  const [confirming, setConfirming] = useState<number | null>(null);
  const [showAll, setShowAll] = useState(false);
  const remove = useMutation({
    mutationFn: (id: number) => api.delete(`/api/body/${id}`),
    onSuccess: async () => {
      setConfirming(null);
      await client.invalidateQueries({ queryKey: ['body'] });
      await client.invalidateQueries({ queryKey: ['today'] });
    },
  });

  if (entries.length === 0 && desktop) {
    return (
      <StatsSection title="Entries">
        <EmptyState
          title="No entries yet"
          action={
            <Button variant="primary" onClick={onAdd}>
              Add entry
            </Button>
          }
        >
          Add your weight, body fat or waist to start the trend.
        </EmptyState>
      </StatsSection>
    );
  }

  if (entries.length === 0) {
    return (
      <Card padding="lg">
        <EmptyState
          title="No entries yet"
          action={
            <Button variant="primary" onClick={onAdd}>
              Add entry
            </Button>
          }
        >
          Add your weight, body fat or waist to start the trend.
        </EmptyState>
      </Card>
    );
  }

  const list = (
    <>
      {remove.isError ? (
        <p role="alert" className="field__hint" style={{ color: 'var(--heart)' }}>
          The entry was not deleted. {errorText(remove.error)}
        </p>
      ) : null}
      <ul>
        {(showAll ? entries : entries.slice(0, ENTRIES_SHOWN)).map((e) => {
          const when = fmtEntryWhen(e.measured_at);
          const asking = confirming === e.id;
          return (
            <li className="plain-row" key={e.id}>
              <span style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0, flex: 1 }}>
                <span>{when}</span>
                <span className="plain-row__sub">{entryWhat(e)}</span>
              </span>
              {asking ? (
                <span role="group" aria-label={`Delete the entry of ${when}?`} style={{ display: 'inline-flex', gap: 8, alignItems: 'center' }}>
                  <Button onClick={() => setConfirming(null)} disabled={remove.isPending}>
                    Keep
                  </Button>
                  <Button variant="primary" onClick={() => remove.mutate(e.id)} disabled={remove.isPending}>
                    {remove.isPending ? 'Deleting' : 'Delete for good'}
                  </Button>
                </span>
              ) : (
                <Button
                  aria-label={`Delete entry of ${when}`}
                  onClick={() => {
                    remove.reset();
                    setConfirming(e.id);
                  }}
                >
                  Delete
                </Button>
              )}
            </li>
          );
        })}
      </ul>
      {!showAll && entries.length > ENTRIES_SHOWN ? (
        <div style={{ padding: '10px 0 12px' }}>
          <Button block onClick={() => setShowAll(true)}>
            Show all ({entries.length})
          </Button>
        </div>
      ) : null}
    </>
  );
  return desktop ? (
    <StatsSection title="Entries" aside={adding ? undefined : <Button variant="primary" onClick={onAdd}>Add entry</Button>}>
      {list}
    </StatsSection>
  ) : (
    <Card padding="list" aria-labelledby="entries-title">
      <h2 id="entries-title" className="card__title card__title--muted" style={{ padding: '10px 0 6px' }}>
        Entries
      </h2>
      {list}
    </Card>
  );
}
