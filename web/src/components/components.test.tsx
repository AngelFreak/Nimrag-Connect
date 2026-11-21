import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { createRef, useState, type ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { routePath } from './MapView';
import {
  AppShell,
  BarSeries,
  Button,
  Card,
  ConfirmDialog,
  EmptyState,
  ErrorState,
  FilterChips,
  Legend,
  navKeyFor,
  ProgressBar,
  Notice,
  Ring,
  SampleDataChip,
  SegmentedControl,
  SettingsGroup,
  SettingsRow,
  Skeleton,
  StatTile,
  StatusBanner,
  SyncButton,
  MapView,
  Table,
  TextField,
  Toggle,
  TopBar,
  Unavailable,
  WatchLink,
  ZoneBars,
  DataList,
  KeyIcon,
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
    // The avatar is a plain link to the profile page.
    expect(screen.getAllByRole('link', { name: 'Profile' })[0]).toHaveAttribute('href', '/profile');
    expect(screen.getAllByRole('button', { name: 'Refresh' }).length).toBeGreaterThan(0);
    expect(screen.getAllByRole('link', { name: 'Watch status unknown' })[0]).toHaveAttribute('href', '/devices');
    const navs = screen.getAllByRole('navigation', { name: 'Main' });
    const bottom = navs.find((n) => n.classList.contains('bottom-nav'));
    expect(bottom).toBeDefined();
    const labels = within(bottom as HTMLElement)
      .getAllByRole('link')
      .map((a) => a.textContent);
    expect(labels).toEqual(['Today', 'Activities', 'Health']);
    expect(within(bottom as HTMLElement).getByRole('link', { name: 'Today' })).toHaveAttribute('aria-current', 'page');
  });

  it('the phone bar has no Strength tab: strength pages light up Activities', () => {
    wrap(
      <AppShell tabs>
        <h1>Strength</h1>
      </AppShell>,
      '/strength',
    );
    const bottom = screen.getAllByRole('navigation', { name: 'Main' }).find((n) => n.classList.contains('bottom-nav'));
    expect(within(bottom as HTMLElement).queryByRole('link', { name: 'Strength' })).toBeNull();
    expect(within(bottom as HTMLElement).getByRole('link', { name: 'Activities' })).toHaveAttribute('aria-current', 'page');
  });

  it('the phone top bar is a landmark, so its buttons are not outside every region', () => {
    wrap(
      <AppShell tabs>
        <h1>Today</h1>
      </AppShell>,
    );
    const bar = document.querySelector('.mainbar') as HTMLElement;
    expect(bar.tagName).toBe('HEADER');
    expect(within(bar).getByRole('link', { name: 'Profile' })).toHaveAttribute('href', '/profile');
  });

  it('the admin area has no tabs, no sync and no watch: only the brand and the profile link', () => {
    wrap(
      <AppShell area="admin">
        <h1>Admin</h1>
      </AppShell>,
      '/admin',
    );
    expect(document.querySelector('.bottom-nav')).toBeNull();
    expect(document.querySelector('.desk-nav')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Refresh' })).toBeNull();
    expect(screen.queryByRole('link', { name: /Watch/ })).toBeNull();
    expect(screen.getByRole('link', { name: 'Nimrag Connect' })).toHaveAttribute('href', '/admin');
    expect(screen.getAllByRole('link', { name: 'Profile' }).length).toBeGreaterThan(0);
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

describe('BarSeries', () => {
  const labels = ['Mon', 'Tue', 'Wed'];

  it('carries its text summary for screen readers, or visibly on request', () => {
    const { rerender } = render(<BarSeries values={[1, 2, 3]} labels={labels} label="Sleep" summary="Sleep per night: 1, 2 and 3 hours." />);
    expect(screen.getByRole('img', { name: 'Sleep' })).toBeInTheDocument();
    expect(screen.getByText('Sleep per night: 1, 2 and 3 hours.')).toHaveClass('sr-only');
    rerender(<BarSeries values={[1, 2, 3]} labels={labels} label="Sleep" summary="Sleep per night: 1, 2 and 3 hours." summaryVisible />);
    expect(screen.getByText('Sleep per night: 1, 2 and 3 hours.')).toHaveClass('chart-summary');
  });

  it('draws the highlighted bar in its own colour', () => {
    render(<BarSeries values={[1, 2, 3]} labels={labels} label="Sleep" highlightIndex={1} highlightColor="rgb(1, 2, 3)" />);
    const bars = Array.from(document.querySelectorAll('svg rect')) as SVGRectElement[];
    expect(bars.map((b) => b.style.fill)).toEqual(['var(--stage-deep)', 'rgb(1, 2, 3)', 'var(--stage-deep)']);
  });

  it('shows "unavailable" when no bar has a value', () => {
    render(<BarSeries values={[null, null]} labels={['a', 'b']} label="None" />);
    expect(screen.getByText('unavailable')).toBeInTheDocument();
  });
});

describe('TopBar back variant with previous and next', () => {
  it('shows both buttons, the next one disabled at the end of the range', () => {
    wrap(<TopBar variant="back" backTo="/health" title="Sleep" prev={{ to: '/sleep/2026-11-17' }} next={null} prevLabel="Previous night" nextLabel="Next night" />);
    expect(screen.getByRole('link', { name: 'Back' })).toHaveAttribute('href', '/health');
    expect(screen.getByRole('link', { name: 'Previous night' })).toHaveAttribute('href', '/sleep/2026-11-17');
    expect(screen.queryByRole('link', { name: 'Next night' })).not.toBeInTheDocument();
  });

  it('a click handler works as a target', async () => {
    const next = vi.fn();
    wrap(<TopBar variant="back" backTo="/" title="Movement" prev={{ to: '/movement/2026-11-18' }} next={{ onClick: next }} />);
    await userEvent.click(screen.getByRole('button', { name: 'Next' }));
    expect(next).toHaveBeenCalledTimes(1);
  });

  it('without prev and next it is the plain back bar', () => {
    wrap(<TopBar variant="back" backTo="/" title="Body" />);
    expect(screen.queryByRole('link', { name: /Previous|Next/ })).not.toBeInTheDocument();
  });
});

describe('Table sorting and className', () => {
  const rows = [{ id: 1, name: 'b' }, { id: 2, name: 'a' }];
  const columns = [
    { key: 'name', header: 'Name', sortable: true, cell: (r: (typeof rows)[number]) => r.name },
    { key: 'id', header: 'Id', cell: (r: (typeof rows)[number]) => r.id },
  ];

  it('sortable headers are buttons with aria-sort; others are plain', async () => {
    const onSort = vi.fn();
    render(<Table caption="Things" className="things" columns={columns} rows={rows} rowKey={(r) => String(r.id)} sort={{ key: 'name', dir: 'asc' }} onSort={onSort} />);
    expect(screen.getByRole('table')).toHaveClass('things');
    expect(screen.getByRole('columnheader', { name: /Name/ })).toHaveAttribute('aria-sort', 'ascending');
    expect(screen.getByRole('columnheader', { name: 'Id' })).not.toHaveAttribute('aria-sort');
    expect(screen.queryByRole('button', { name: 'Id' })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /Name/ }));
    expect(onSort).toHaveBeenCalledWith('name');
  });

  it('without onSort the headers stay plain text', () => {
    render(<Table caption="Things" columns={columns} rows={rows} rowKey={(r) => String(r.id)} />);
    expect(screen.queryByRole('button', { name: /Name/ })).not.toBeInTheDocument();
  });
});

describe('StatTile as a link', () => {
  it('links the whole tile and passes data attributes to the link', () => {
    wrap(<StatTile label="Steps" value="6,240" href="/movement" data-card="steps" />);
    const link = screen.getByRole('link', { name: /Steps/ });
    expect(link).toHaveAttribute('href', '/movement');
    expect(link).toHaveAttribute('data-card', 'steps');
  });

  it('is not a link without href', () => {
    render(<StatTile label="Steps" value="6,240" />);
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });
});

describe('Legend swatches', () => {
  it('draws the shape the chart uses', () => {
    render(
      <Legend
        items={[
          { key: 'bar', label: 'Bars', color: 'red' },
          { key: 'line', label: 'Line', color: 'blue', shape: 'line' },
          { key: 'goal', label: 'Goal', color: 'green', shape: 'dash' },
          { key: 'dot', label: 'Readings', color: 'gray', shape: 'dot' },
        ]}
      />,
    );
    const swatches = Array.from(document.querySelectorAll('.swatch'));
    expect(swatches.map((s) => s.className)).toEqual(['swatch', 'swatch swatch--line', 'swatch swatch--dash', 'swatch swatch--dot']);
    expect(swatches.every((s) => s.getAttribute('aria-hidden') === 'true')).toBe(true);
  });
});

describe('TextField', () => {
  it('hands its ref to the input', () => {
    const ref = createRef<HTMLInputElement>();
    render(<TextField ref={ref} label="Weight" />);
    expect(ref.current).toBe(screen.getByLabelText('Weight'));
    ref.current?.focus();
    expect(screen.getByLabelText('Weight')).toHaveFocus();
  });
});

describe('navKeyFor', () => {
  it('does not highlight Overview on Calendar', () => {
    expect(navKeyFor('/calendar', '')).toBe('health');
    expect(navKeyFor('/calendar/2026-11', '')).toBe('health');
    expect(navKeyFor('/', '')).toBe('today');
    expect(navKeyFor('/day/2026-11-18', '')).toBe('today');
  });

  it('separates Strength from Activities and knows Movement and Devices', () => {
    expect(navKeyFor('/strength', '')).toBe('strength');
    expect(navKeyFor('/activities', '?type=strength')).toBe('strength');
    expect(navKeyFor('/activities/9/strength', '')).toBe('strength');
    expect(navKeyFor('/activities/9', '')).toBe('activities');
    expect(navKeyFor('/movement/2026-11-18', '')).toBe('health');
    expect(navKeyFor('/devices', '')).toBe('devices');
    expect(navKeyFor('/profile', '')).toBe('other');
  });
});

describe('Table in a narrow box', () => {
  it('labels every cell with its column name so a wide table can stack, and leaves an actions column unlabelled', () => {
    const rows = [{ id: 1, name: 'pixel8a', used: 'Never' }];
    render(
      <Table
        caption="Phones"
        wide
        columns={[
          { key: 'name', header: 'Name', cell: (r: (typeof rows)[number]) => r.name },
          { key: 'used', header: 'Last used', cell: (r: (typeof rows)[number]) => r.used },
          { key: 'act', header: <span className="sr-only">Actions</span>, cell: () => <button type="button">Revoke</button> },
        ]}
        rows={rows}
        rowKey={(r) => String(r.id)}
      />,
    );
    expect(screen.getByRole('table')).toHaveClass('table--wide');
    expect(screen.getByRole('cell', { name: 'pixel8a' })).toHaveAttribute('data-label', 'Name');
    expect(screen.getByRole('cell', { name: 'Never' })).toHaveAttribute('data-label', 'Last used');
    expect(screen.getByRole('button', { name: 'Revoke' }).closest('td')).not.toHaveAttribute('data-label');
  });
});

describe('Notice', () => {
  it('Notice is plain text, not an alert; the warning tone adds an icon', () => {
    const { container } = render(<Notice tone="warn">The login is unofficial.</Notice>);
    expect(screen.getByText('The login is unofficial.')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(container.querySelector('svg')).not.toBeNull();
  });
});

describe('StatTile and DataList hide what has no value', () => {
  it('StatTile hideWhenMissing renders nothing for null, undefined, NaN and an empty string', () => {
    const { container } = render(
      <>
        <StatTile label="Cadence" value={null} hideWhenMissing />
        <StatTile label="VO2" value={undefined} hideWhenMissing />
        <StatTile label="Pace" value={Number.NaN} hideWhenMissing />
        <StatTile label="Time" value="" hideWhenMissing />
      </>,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('StatTile hideWhenMissing keeps a tile with a value, and a real zero', () => {
    render(
      <>
        <StatTile label="Distance" value="6.4 km" hideWhenMissing />
        <StatTile label="Climb" value={0} hideWhenMissing />
      </>,
    );
    expect(screen.getByText('6.4 km')).toBeInTheDocument();
    expect(screen.getByText('Climb')).toBeInTheDocument();
  });

  it('DataList: optional rows and hideMissing leave out empty rows, other rows still say "unavailable"', () => {
    render(
      <DataList
        items={[
          { label: 'Model', value: null, optional: true },
          { label: 'Firmware', value: '21.3', optional: true },
          { label: 'Battery', value: null },
        ]}
      />,
    );
    expect(screen.queryByText('Model')).not.toBeInTheDocument();
    expect(screen.getByText('21.3')).toBeInTheDocument();
    expect(screen.getByText('Battery').nextElementSibling).toHaveTextContent('unavailable');
  });

  it('DataList hideMissing shows the empty text when nothing is left, and nothing without it', () => {
    const { container, rerender } = render(<DataList hideMissing empty="No data." items={[{ label: 'Steps', value: null }]} />);
    expect(screen.getByText('No data.')).toBeInTheDocument();
    rerender(<DataList hideMissing items={[{ label: 'Steps', value: null }]} />);
    expect(container).toBeEmptyDOMElement();
  });
});

describe('Toggle', () => {
  it('is a labelled switch that follows the checkbox state and keeps no native look', async () => {
    function Host() {
      const [on, setOn] = useState(false);
      return <Toggle label="Overnight averages" checked={on} onChange={(e) => setOn(e.target.checked)} />;
    }
    render(<Host />);
    const sw = screen.getByRole('switch', { name: 'Overnight averages' });
    expect(sw).not.toBeChecked();
    await userEvent.click(sw);
    expect(sw).toBeChecked();
    expect(sw).toHaveClass('toggle__input');
  });
});

describe('MapView without tiles', () => {
  it('draws the route placeholder without any developer caption', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('<html></html>', { status: 200, headers: { 'content-type': 'text/html' } })));
    render(<MapView tilesUrl="/tiles/none.pmtiles" />);
    expect(await screen.findByRole('img', { name: 'Route map' })).toBeInTheDocument();
    expect(screen.queryByText(/placeholder|your own server/i)).not.toBeInTheDocument();
    vi.unstubAllGlobals();
  });
});

describe('routePath', () => {
  it('draws nothing without a track', () => {
    expect(routePath(undefined, 350, 150)).toBeNull();
    expect(routePath([[10, 56]], 350, 150)).toBeNull();
  });
  it('fits the real track inside the box, start first', () => {
    const r = routePath([[10, 56], [10.01, 56], [10.01, 56.01]], 350, 150);
    expect(r).not.toBeNull();
    const nums = (r?.d.match(/-?\d+(\.\d+)?/g) ?? []).map(Number);
    for (let i = 0; i < nums.length; i += 2) {
      expect(nums[i]).toBeGreaterThanOrEqual(0);
      expect(nums[i]).toBeLessThanOrEqual(350);
      expect(nums[i + 1]).toBeGreaterThanOrEqual(0);
      expect(nums[i + 1]).toBeLessThanOrEqual(150);
    }
    expect(r?.d.startsWith(`M${r.start[0].toFixed(1)} ${r.start[1].toFixed(1)}`)).toBe(true);
  });
});

describe('SettingsRow and SettingsGroup', () => {
  it('a link row is one link named by its title and value, with the icon hidden from assistive tech', () => {
    wrap(<SettingsRow icon={<KeyIcon />} title="Timezone" value="Europe/Copenhagen" to="/profile/timezone" />);
    const link = screen.getByRole('link', { name: 'Timezone Europe/Copenhagen' });
    expect(link).toHaveAttribute('href', '/profile/timezone');
    expect(link.querySelectorAll('svg[aria-hidden="true"]').length).toBeGreaterThanOrEqual(2);
  });

  it('a row without a value is named by its title alone', () => {
    wrap(<SettingsRow icon={<KeyIcon />} title="Password" to="/profile/password" />);
    expect(screen.getByRole('link', { name: 'Password' })).toBeInTheDocument();
  });

  it('a button row runs its action, and has no chevron when it says so', async () => {
    const onClick = vi.fn();
    const { container } = wrap(<SettingsRow icon={<KeyIcon />} title="Log out" chevron={false} onClick={onClick} />);
    await userEvent.click(screen.getByRole('button', { name: 'Log out' }));
    expect(onClick).toHaveBeenCalledTimes(1);
    // Only the icon tile's svg: no chevron.
    expect(container.querySelectorAll('svg')).toHaveLength(1);
  });

  it('a switch row is a switch in a label: pressing the row toggles it', async () => {
    const onChange = vi.fn();
    wrap(<SettingsRow icon={<KeyIcon />} title="Send activities" value="Off" checked={false} onCheckedChange={onChange} />);
    const toggle = screen.getByRole('switch', { name: 'Send activities Off' });
    expect(toggle).not.toBeChecked();
    await userEvent.click(screen.getByText('Send activities'));
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it('an action row keeps its small button and is not itself a control', async () => {
    const onClick = vi.fn();
    wrap(<SettingsRow icon={<KeyIcon />} title="Pixel 9a" value="Signed in 2 Nov" action={<Button onClick={onClick}>Sign out</Button>} />);
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(onClick).toHaveBeenCalled();
  });

  it('an info row and an external row: a fact, and a link that opens a new tab', () => {
    wrap(
      <>
        <SettingsRow icon={<KeyIcon />} title="Address" value="nimrag.example" info />
        <SettingsRow icon={<KeyIcon />} title="Source code" value="github.com/x/y" href="https://github.com/x/y" />
      </>,
    );
    expect(screen.getByText('nimrag.example')).toBeInTheDocument();
    const link = screen.getByRole('link', { name: 'Source code github.com/x/y' });
    expect(link).toHaveAttribute('href', 'https://github.com/x/y');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', expect.stringContaining('noopener'));
  });

  it('a download row is a same-tab link with the download attribute', () => {
    wrap(<SettingsRow icon={<KeyIcon />} title="Download my data" download="/api/export" />);
    const link = screen.getByRole('link', { name: 'Download my data' });
    expect(link).toHaveAttribute('href', '/api/export');
    expect(link).toHaveAttribute('download');
    expect(link).not.toHaveAttribute('target');
  });

  it('a group is a named section with its rows in one list, and renders nothing when empty', () => {
    const { container, rerender } = wrap(
      <SettingsGroup title="Account">
        <SettingsRow icon={<KeyIcon />} title="Password" to="/p" />
        {null}
        <SettingsRow icon={<KeyIcon />} title="Phones" to="/q" />
      </SettingsGroup>,
    );
    const group = screen.getByRole('region', { name: 'Account' });
    expect(within(group).getByRole('heading', { level: 2, name: 'Account' })).toBeInTheDocument();
    expect(within(group).getAllByRole('listitem')).toHaveLength(2);
    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <SettingsGroup title="Empty">{null}</SettingsGroup>
        </MemoryRouter>
      </QueryClientProvider>,
    );
    expect(container.querySelector('.sgroup')).toBeNull();
  });
});

describe('StatusBanner', () => {
  it('is one line and one fix, and not an alert', async () => {
    const fix = vi.fn();
    render(<StatusBanner action={<Button onClick={fix}>Turn on</Button>}>Bluetooth is off.</StatusBanner>);
    expect(screen.getByText('Bluetooth is off.')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Turn on' }));
    expect(fix).toHaveBeenCalled();
  });

  it('works without an action', () => {
    render(<StatusBanner tone="info">Heads up.</StatusBanner>);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
});

describe('ConfirmDialog', () => {
  function Harness({ onConfirm = vi.fn(), pending = false, error }: { onConfirm?: () => void; pending?: boolean; error?: string }) {
    const [open, setOpen] = useState(false);
    return (
      <>
        <button onClick={() => setOpen(true)}>Open</button>
        {open ? (
          <ConfirmDialog title="Log out?" consequence="You will need a code to log in again." confirmLabel="Log out" pending={pending} error={error} onConfirm={onConfirm} onCancel={() => setOpen(false)} />
        ) : null}
      </>
    );
  }

  it('is a modal dialog named by its question and described by its consequence', async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    const dialog = screen.getByRole('dialog', { name: 'Log out?' });
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAccessibleDescription('You will need a code to log in again.');
    expect(within(dialog).getByRole('button', { name: 'Log out' })).toBeInTheDocument();
  });

  it('keeps Tab inside, Escape cancels, and focus returns to what opened it', async () => {
    const u = userEvent.setup();
    render(<Harness />);
    const opener = screen.getByRole('button', { name: 'Open' });
    await u.click(opener);
    const cancel = screen.getByRole('button', { name: 'Cancel' });
    const yes = screen.getByRole('button', { name: 'Log out' });
    cancel.focus();
    await u.tab();
    expect(yes).toHaveFocus();
    await u.tab();
    expect(cancel).toHaveFocus();
    await u.tab({ shift: true });
    expect(yes).toHaveFocus();
    await u.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it('confirm runs the action; while it runs both buttons wait and Escape does nothing', async () => {
    const u = userEvent.setup();
    const onConfirm = vi.fn();
    const { rerender } = render(<Harness onConfirm={onConfirm} />);
    await u.click(screen.getByRole('button', { name: 'Open' }));
    await u.click(screen.getByRole('button', { name: 'Log out' }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    rerender(<Harness onConfirm={onConfirm} pending />);
    expect(screen.getByRole('button', { name: 'Log out' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled();
    await u.keyboard('{Escape}');
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });

  it('shows why a try failed and stays open', async () => {
    render(<Harness error="Could not reach the server." />);
    await userEvent.click(screen.getByRole('button', { name: 'Open' }));
    expect(screen.getByRole('alert')).toHaveTextContent('Could not reach the server.');
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });
});
