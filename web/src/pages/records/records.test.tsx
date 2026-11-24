import { render, screen, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RecordsResponse } from '../../api/generated';
import type { PersonalRecord } from '../../api/generated/PersonalRecord';
import type { RecordGroup } from '../../api/generated/RecordGroup';
import { RecordBadge } from './RecordBadge';
import { RecordsBody, RecordsPage } from './RecordsPage';
import { badgeText, fmtRecordDate, fmtRecordValue, groupHasRecords, recordLabel } from './format';

const ID = 'a'.repeat(64);

function rec(over: Partial<PersonalRecord> & Pick<PersonalRecord, 'kind' | 'unit'>): PersonalRecord {
  return { value: null, date: null, activity: null, previous: null, ...over };
}

const run = (over: Partial<RecordGroup> = {}): RecordGroup => ({
  key: 'run',
  records: [
    rec({
      kind: 'run_5k',
      unit: 's',
      value: 1503.4,
      date: '2026-11-11',
      activity: { id: ID, kind: 'run', sport_name: 'running', start: '2026-11-11T08:00:00+01:00' },
      previous: { value: 1650, date: '2026-11-02' },
    }),
    rec({ kind: 'run_marathon', unit: 's' }),
    rec({ kind: 'run_longest', unit: 'm', value: 10000, date: '2026-11-11', activity: { id: ID, kind: 'run', sport_name: 'running', start: '2026-11-11T08:00:00+01:00' } }),
  ],
  ...over,
});

function data(groups: RecordGroup[]): RecordsResponse {
  return { sample: false, groups };
}

describe('record formatting', () => {
  it('shows times as m:ss or h:mm:ss, distances in km with two decimals, ascent in metres and steps with separators', () => {
    expect(fmtRecordValue({ kind: 'run_5k', unit: 's', value: 1503.4 })).toBe('25:03');
    expect(fmtRecordValue({ kind: 'run_half', unit: 's', value: 5400 })).toBe('1:30:00');
    expect(fmtRecordValue({ kind: 'run_400m', unit: 's', value: 80 })).toBe('1:20');
    expect(fmtRecordValue({ kind: 'run_longest', unit: 'm', value: 21097.5 })).toBe('21.10 km');
    expect(fmtRecordValue({ kind: 'run_longest', unit: 'm', value: 800 })).toBe('0.80 km');
    expect(fmtRecordValue({ kind: 'run_ascent', unit: 'm', value: 412 })).toBe('412 m');
    expect(fmtRecordValue({ kind: 'steps_day', unit: 'steps', value: 23456 })).toBe('23,456');
    expect(fmtRecordValue({ kind: 'steps_day', unit: 'steps', value: null })).toBeNull();
  });

  it('names dates, with the year only when it is not this year, and a week by its Monday', () => {
    expect(fmtRecordDate('2026-11-11', 'run_5k', '2026-11-24')).toBe('Wed 11 Nov');
    expect(fmtRecordDate('2025-03-02', 'run_5k', '2026-11-24')).toBe('Sun 2 Mar 2025');
    expect(fmtRecordDate('2026-11-02', 'steps_week', '2026-11-24')).toBe('Week of 2 Nov');
    expect(fmtRecordDate(null, 'run_5k', '2026-11-24')).toBeNull();
  });

  it('has a label for every kind the server sends and a badge text that lists them', () => {
    expect(recordLabel('run_half')).toBe('Fastest half marathon');
    expect(recordLabel('steps_week')).toBe('Most steps in a week');
    expect(badgeText([])).toBeNull();
    expect(badgeText(['run_5k'])).toBe('Personal record: 5 km');
    expect(badgeText(['run_5k', 'run_10k'])).toBe('Personal records: 5 km, 10 km');
  });

  it('counts a group as empty when no record has a value', () => {
    expect(groupHasRecords([{ value: null }, { value: null }])).toBe(false);
    expect(groupHasRecords([{ value: null }, { value: 0.5 }])).toBe(true);
  });
});

describe('RecordsBody', () => {
  const renderBody = (d: RecordsResponse) =>
    render(
      <MemoryRouter>
        <RecordsBody data={d} today="2026-11-24" />
      </MemoryRouter>,
    );

  it('lists each record with its value, date, sport and previous best, linking to the activity', () => {
    renderBody(data([run()]));
    const card = screen.getByRole('region', { name: 'Running' });
    const rows = within(card).getAllByRole('listitem');
    expect(rows).toHaveLength(3);
    const link = within(rows[0]!).getByRole('link');
    expect(link).toHaveAttribute('href', `/activities/${ID}`);
    expect(link).toHaveTextContent('Fastest 5 km');
    expect(link).toHaveTextContent('Wed 11 Nov · Run');
    expect(link).toHaveTextContent('25:03');
    expect(link).toHaveTextContent('Before: 27:30 · Mon 2 Nov');
  });

  it('says "Not yet" for a record nothing counts for, without a link', () => {
    renderBody(data([run()]));
    const row = screen.getByText('Fastest marathon').closest('li')!;
    expect(within(row).getByText('Not yet')).toBeInTheDocument();
    expect(within(row).queryByRole('link')).toBeNull();
  });

  it('shows an empty state for a sport without records and the step records without a link', () => {
    renderBody(
      data([
        { key: 'cycle', records: [rec({ kind: 'cycle_longest', unit: 'm' }), rec({ kind: 'cycle_20k', unit: 's' })] },
        {
          key: 'steps',
          records: [rec({ kind: 'steps_day', unit: 'steps', value: 23456, date: '2026-11-16' }), rec({ kind: 'steps_week', unit: 'steps', value: 101234, date: '2026-11-09' })],
        },
      ]),
    );
    expect(screen.getByText('No rides yet')).toBeInTheDocument();
    expect(screen.queryByText('Fastest 20 km')).toBeNull();
    const steps = screen.getByRole('region', { name: 'Steps' });
    expect(within(steps).getByText('23,456')).toBeInTheDocument();
    expect(within(steps).getByText('Mon 16 Nov')).toBeInTheDocument();
    expect(within(steps).getByText('Week of 9 Nov')).toBeInTheDocument();
    expect(within(steps).queryByRole('link')).toBeNull();
  });
});

describe('RecordBadge', () => {
  it('links to the records page and says what the activity holds; nothing for no records', () => {
    const { container, rerender } = render(
      <MemoryRouter>
        <RecordBadge kinds={['run_5k', 'run_10k']} />
      </MemoryRouter>,
    );
    expect(screen.getByRole('link', { name: 'Personal records: 5 km, 10 km' })).toHaveAttribute('href', '/records');
    rerender(
      <MemoryRouter>
        <RecordBadge kinds={[]} />
      </MemoryRouter>,
    );
    expect(container).toBeEmptyDOMElement();
  });
});

describe('RecordsPage', () => {
  beforeEach(() => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        const url = new URL(String(input), 'http://localhost');
        if (url.pathname === '/api/records') {
          return new Response(JSON.stringify(data([run()])), { status: 200, headers: { 'content-type': 'application/json' } });
        }
        return new Response('{}', { status: 404, headers: { 'content-type': 'application/json' } });
      }),
    );
  });
  afterEach(() => vi.unstubAllGlobals());

  it('loads the records from the API', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <RecordsPage />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    expect(await screen.findByText('Fastest 5 km')).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 1, name: 'Records' })).toBeInTheDocument();
  });
});
