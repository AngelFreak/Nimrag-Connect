import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BodyPage } from '../BodyPage';
import { CalendarPage } from '../CalendarPage';
import { MovementPage } from '../MovementPage';
import { TrainingPage } from '../TrainingPage';
import { body, calendar, mockApi, movement, renderAt, TODAY, training } from './testUtils';

// Training, Movement, Body and Calendar at >= 1024 px: a title (and toolbar) header, sections on the
// sheet instead of cards, the figures in strips. The phone layout is covered by pages.test.tsx.
vi.mock('uplot', () => ({
  default: class {
    width = 320;
    setSize() {}
    destroy() {}
  },
}));

const real = window.matchMedia;
beforeEach(() => {
  window.matchMedia = ((query: string) => ({ matches: query.includes('min-width: 1024px'), media: query, addEventListener: () => {}, removeEventListener: () => {} })) as unknown as typeof window.matchMedia;
});
afterEach(() => {
  window.matchMedia = real;
  vi.unstubAllGlobals();
});

describe('wide screen', () => {
  it('Training: recovery, status and VO2 max in the left column, load and intensity beside them', async () => {
    mockApi({ '/api/training': training({ sample: true }) });
    renderAt(<TrainingPage />, '/training');
    expect(await screen.findByRole('heading', { level: 1, name: 'Training' })).toBeInTheDocument();
    const side = (await waitForColumn('.stats-side')) as HTMLElement;
    expect(within(side).getByRole('heading', { name: 'Recovery Time' })).toBeInTheDocument();
    expect(within(side).getByRole('heading', { name: 'VO2 Max (derived)' })).toBeInTheDocument();
    expect(within(side).getByText(/not the watch's own VO2 max figure/)).toBeInTheDocument();
    const main = document.querySelector('.stats-main') as HTMLElement;
    expect(within(main).getByRole('heading', { name: '7-day Load' })).toBeInTheDocument();
    expect(within(main).getByRole('heading', { name: 'Load by Intensity' })).toBeInTheDocument();
    expect(document.querySelector('.card')).toBeNull();
  });

  it('Movement: a day toolbar, steps against the goal on the left, the week and intensity on the right', async () => {
    mockApi({ '/api/today': TODAY, '/api/movement/2026-11-19': movement() });
    renderAt(<MovementPage />, '/movement', '/movement/:date?');
    expect(await screen.findByRole('heading', { level: 1, name: 'Movement' })).toBeInTheDocument();
    expect(await screen.findByText('6,240')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: '6,240 of 8,000 steps, 78 percent' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Steps this week' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Next day' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Previous day' }));
    expect(screen.getByTestId('where')).toHaveTextContent('/movement/2026-11-18');
  });

  it('Body: the chart and a strip of the latest figures, entries with a delete, Add entry beside the title of the list', async () => {
    mockApi({ '/api/today': TODAY, '/api/body': body() });
    renderAt(<BodyPage />, '/body');
    expect(await screen.findByRole('heading', { level: 1, name: 'Weight & Body' })).toBeInTheDocument();
    expect(await screen.findByText('80.8 kg')).toBeInTheDocument();
    expect(screen.getByText('−1.4 kg')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Entries' })).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: /^Delete entry of / })).toHaveLength(2);
    // The sticky bar of the phone is not used: the button sits with the entries.
    expect(document.querySelector('.footer-bar')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Add entry' }));
    expect(await screen.findByRole('button', { name: 'Save entry' })).toBeInTheDocument();
  });

  it('Calendar: the month in a chip with arrows, the grid on the sheet, the focused day and month totals at the side', async () => {
    mockApi({ '/api/today': TODAY, '/api/calendar/2026-11': calendar() });
    renderAt(<CalendarPage />, '/calendar', '/calendar/:month?');
    expect(await screen.findByRole('heading', { level: 1, name: 'Calendar' })).toBeInTheDocument();
    expect(await screen.findByRole('link', { name: '5 November, run' })).toHaveAttribute('href', '/day/2026-11-05');
    expect(screen.getByText('November 2026')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Month so far' })).toBeInTheDocument();
    expect(document.querySelector('.cal-sheet')).not.toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Next month' }));
    expect(screen.getByTestId('where')).toHaveTextContent('/calendar/2026-12');
  });
});

async function waitForColumn(selector: string) {
  await screen.findByRole('heading', { name: '7-day Load' });
  return document.querySelector(selector);
}
