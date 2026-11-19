import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { useState, type ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import {
  AppShell,
  Card,
  EmptyState,
  ErrorState,
  FilterChips,
  ProgressBar,
  Ring,
  SampleDataChip,
  SegmentedControl,
  Skeleton,
  StatTile,
  SyncButton,
  Table,
  TopBar,
  Unavailable,
  WatchLink,
  ZoneBars,
  DataList,
} from './index';

function wrap(ui: ReactNode, path = '/') {
  const client = new QueryClient();
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>{ui}</MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('Unavailable (hard rule: missing metrics are never 0)', () => {
  it('shows the word "unavailable"', () => {
    render(<Unavailable />);
    expect(screen.getByText('unavailable')).toBeInTheDocument();
  });

  it('StatTile shows "unavailable" for null and undefined, and never 0', () => {
    render(
      <>
        <StatTile label="HRV" value={null} />
        <StatTile label="Pace" value={undefined} />
      </>,
    );
    expect(screen.getAllByText('unavailable')).toHaveLength(2);
    expect(screen.queryByText('0')).not.toBeInTheDocument();
  });

  it('a real zero is still shown as 0', () => {
    render(<StatTile label="Sets" value="0" />);
    expect(screen.getByText('0')).toBeInTheDocument();
    expect(screen.queryByText('unavailable')).not.toBeInTheDocument();
  });

  it('DataList, Table and ZoneBars show "unavailable" for missing values', () => {
    render(
      <>
        <DataList items={[{ label: 'Firmware', value: null }]} />
        <Table caption="Laps" rows={[{ id: 1, hr: null as string | null }]} rowKey={(r) => String(r.id)} columns={[{ key: 'hr', header: 'HR', cell: (r) => r.hr }]} />
        <ZoneBars zones={[{ zone: 1, minutes: null }]} />
      </>,
    );
    expect(screen.getAllByText('unavailable')).toHaveLength(3);
  });

  it('Ring with no value says unavailable in its accessible name', () => {
    render(<Ring value={null} label="Energy level" />);
    expect(screen.getByRole('img', { name: 'Energy level unavailable' })).toBeInTheDocument();
  });
});

describe('components render', () => {
  it('Card renders a section with its content', () => {
    render(<Card aria-label="Energy">content</Card>);
    expect(screen.getByRole('region', { name: 'Energy' })).toHaveTextContent('content');
  });

  it('StatTile renders label, value and sub line', () => {
    render(<StatTile label="Sleep" value="7h 12m" sub="Score 81" tone="sleep" />);
    expect(screen.getByText('Sleep')).toBeInTheDocument();
    expect(screen.getByText('7h 12m')).toBeInTheDocument();
    expect(screen.getByText('Score 81')).toHaveClass('tone-sleep');
  });

  it('Ring and ProgressBar carry accessible numbers', () => {
    render(
      <>
        <Ring value={68} label="Energy level" />
        <ProgressBar value={6240} max={8000} label="6,240 of 8,000 steps" />
      </>,
    );
    expect(screen.getByRole('img', { name: 'Energy level 68 out of 100' })).toBeInTheDocument();
    const bar = screen.getByRole('progressbar', { name: '6,240 of 8,000 steps' });
    expect(bar).toHaveAttribute('aria-valuenow', '6240');
  });

  it('ZoneBars lists zone 5 first', () => {
    render(<ZoneBars zones={[{ zone: 1, minutes: 2 }, { zone: 5, minutes: 1 }]} />);
    const items = within(screen.getByRole('list', { name: 'Time in heart rate zones' })).getAllByRole('listitem');
    expect(items[0]).toHaveTextContent('Zone 5');
    expect(items[1]).toHaveTextContent('Zone 1');
  });

  it('SampleDataChip shows only when sample is true', () => {
    const { rerender } = render(<SampleDataChip sample />);
    expect(screen.getByText('Sample data')).toBeInTheDocument();
    rerender(<SampleDataChip sample={false} />);
    expect(screen.queryByText('Sample data')).not.toBeInTheDocument();
    rerender(<SampleDataChip sample={undefined} />);
    expect(screen.queryByText('Sample data')).not.toBeInTheDocument();
  });

  it('EmptyState, ErrorState and Skeleton', () => {
    render(
      <>
        <EmptyState title="No activities yet">Sync the watch.</EmptyState>
        <ErrorState title="Could not load">Try again.</ErrorState>
        <Skeleton />
      </>,
    );
    expect(screen.getByText('No activities yet')).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('Could not load');
  });

  it('Table has real column headers', () => {
    render(<Table caption="Laps" rows={[{ n: 1 }]} rowKey={(r) => String(r.n)} columns={[{ key: 'n', header: 'Lap', cell: (r) => r.n }]} />);
    expect(screen.getByRole('columnheader', { name: 'Lap' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: '1' })).toBeInTheDocument();
  });
});

describe('TopBar', () => {
  it('large variant: one h1 with the subtitle', () => {
    wrap(<TopBar variant="large" subtitle="Thursday 19 November" title="Today" />);
    expect(screen.getByRole('heading', { level: 1, name: 'Today' })).toBeInTheDocument();
    expect(screen.getByText('Thursday 19 November')).toBeInTheDocument();
  });

  it('back variant: a labelled link back', () => {
    wrap(<TopBar variant="back" backTo="/activities" title="Run" subtitle="Mon 16 Nov" />);
    expect(screen.getByRole('link', { name: 'Back' })).toHaveAttribute('href', '/activities');
  });

  it('pager variant: previous and next, next hidden at the end of the range', () => {
    wrap(<TopBar variant="pager" title="Wed 18 Nov" prevLabel="Previous day" nextLabel="Next day" prev={{ to: '/day/2026-11-17' }} next={null} />);
    expect(screen.getByRole('link', { name: 'Previous day' })).toHaveAttribute('href', '/day/2026-11-17');
    expect(screen.queryByRole('link', { name: 'Next day' })).not.toBeInTheDocument();
  });
});

describe('SegmentedControl keyboard', () => {
  function Harness({ onChange }: { onChange?: (v: string) => void }) {
    const [v, setV] = useState<'7d' | '4w' | '3m'>('4w');
    return (
      <SegmentedControl
        label="Time range"
        options={[
          { value: '7d', label: '7 days' },
          { value: '4w', label: '4 weeks' },
          { value: '3m', label: '3 months' },
        ]}
        value={v}
        onChange={(n) => {
          setV(n);
          onChange?.(n);
        }}
      />
    );
  }

  it('exposes a named group with aria-pressed on the selected option', () => {
    render(<Harness />);
    const group = screen.getByRole('group', { name: 'Time range' });
    expect(within(group).getByRole('button', { name: '4 weeks' })).toHaveAttribute('aria-pressed', 'true');
    expect(within(group).getByRole('button', { name: '7 days' })).toHaveAttribute('aria-pressed', 'false');
  });

  it('Tab reaches it, arrows move focus, Enter and Space select', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    await user.tab();
    expect(screen.getByRole('button', { name: '7 days' })).toHaveFocus();
    await user.keyboard('{ArrowRight}');
    expect(screen.getByRole('button', { name: '4 weeks' })).toHaveFocus();
    await user.keyboard('{ArrowRight}');
    expect(screen.getByRole('button', { name: '3 months' })).toHaveFocus();
    await user.keyboard('{Enter}');
    expect(onChange).toHaveBeenLastCalledWith('3m');
    expect(screen.getByRole('button', { name: '3 months' })).toHaveAttribute('aria-pressed', 'true');
    await user.keyboard('{Home}');
    expect(screen.getByRole('button', { name: '7 days' })).toHaveFocus();
    await user.keyboard(' ');
    expect(onChange).toHaveBeenLastCalledWith('7d');
    await user.keyboard('{End}');
    expect(screen.getByRole('button', { name: '3 months' })).toHaveFocus();
  });
});

describe('FilterChips', () => {
  it('marks the selected chip with aria-pressed and reports clicks', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <FilterChips
        label="Filter by type"
        options={[
          { value: 'all', label: 'All' },
          { value: 'run', label: 'Run' },
        ]}
        value="all"
        onChange={onChange}
      />,
    );
    expect(screen.getByRole('button', { name: 'All' })).toHaveAttribute('aria-pressed', 'true');
    await user.click(screen.getByRole('button', { name: 'Run' }));
    expect(onChange).toHaveBeenCalledWith('run');
  });
});

describe('top bar buttons', () => {
  it('SyncButton: Refresh in a browser, Sync now in the app, busy while spinning', () => {
    const { rerender } = render(<SyncButton spinning={false} onClick={() => {}} />);
    expect(screen.getByRole('button', { name: 'Refresh' })).toHaveAttribute('aria-busy', 'false');
    rerender(<SyncButton spinning native onClick={() => {}} />);
    expect(screen.getByRole('button', { name: 'Sync now' })).toHaveAttribute('aria-busy', 'true');
  });

  it('WatchLink links to /devices and says the state in words', () => {
    const { rerender } = wrap(<WatchLink link="connected" />);
    expect(screen.getByRole('link', { name: 'Watch connected' })).toHaveAttribute('href', '/devices');
    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <WatchLink link="problem" />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    expect(screen.getByRole('link', { name: 'Watch connection problem' })).toBeInTheDocument();
  });
});

describe('AppShell', () => {
  it('main tabs: profile, sync, watch and the bottom navigation exactly as drawn', () => {
    wrap(
      <AppShell tabs>
        <h1>Today</h1>
      </AppShell>,
    );
    expect(screen.getAllByRole('link', { name: 'Profile and settings' })[0]).toHaveAttribute('href', '/settings');
    expect(screen.getAllByRole('button', { name: 'Refresh' }).length).toBeGreaterThan(0);
    expect(screen.getAllByRole('link', { name: 'Watch status unknown' })[0]).toHaveAttribute('href', '/devices');
    const navs = screen.getAllByRole('navigation', { name: 'Main' });
    const bottom = navs.find((n) => n.classList.contains('bottom-nav'));
    expect(bottom).toBeDefined();
    const labels = within(bottom as HTMLElement)
      .getAllByRole('link')
      .map((a) => a.textContent);
    expect(labels).toEqual(['Today', 'Activities', 'Health', 'Strength']);
    expect(within(bottom as HTMLElement).getByRole('link', { name: 'Today' })).toHaveAttribute('aria-current', 'page');
  });

  it('detail pages have no bottom navigation', () => {
    wrap(
      <AppShell>
        <h1>Sleep</h1>
      </AppShell>,
      '/sleep',
    );
    expect(document.querySelector('.bottom-nav')).toBeNull();
  });
});
