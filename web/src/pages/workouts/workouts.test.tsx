import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { SendInfo, WorkoutBody, WorkoutDetail, WorkoutList, WorkoutSummary } from '../../api/generated';
import { ActivitiesPage } from '../ActivitiesPage';
import { WorkoutPage } from '../WorkoutPage';
import { json, mockApi } from '../body/testUtils';
import { DEFAULT_LIMITS } from './model';
import { WorkoutListPage } from './WorkoutListPage';

afterEach(() => vi.unstubAllGlobals());

const NOW = '2026-11-19T08:00:00+01:00';
const idle: SendInfo = { state: 'idle', requested_at: null, finished_at: null, error: null };
const queued: SendInfo = { state: 'queued', requested_at: NOW, finished_at: null, error: null };
const failed: SendInfo = { state: 'failed', requested_at: NOW, finished_at: NOW, error: 'The watch refused the workout while creating the file (status 2, NO_SPACE)' };

const BODY: WorkoutBody = {
  name: 'Tempo 20',
  sport: 'running',
  nodes: [
    { type: 'step', intensity: 'warmup', duration: { kind: 'time', seconds: 600 }, target: { kind: 'none' } },
    { type: 'step', intensity: 'active', duration: { kind: 'time', seconds: 1200 }, target: { kind: 'heart_rate_zone', zone: 3 } },
  ],
};

const summary = (over: Partial<WorkoutSummary> = {}): WorkoutSummary => ({
  id: 7,
  name: 'Tempo 20',
  sport: 'running',
  steps: 2,
  duration_s: 1800,
  updated_at: NOW,
  send: idle,
  ...over,
});

const list = (over: Partial<WorkoutList> = {}): WorkoutList => ({
  watch_send: { available: true, reason: null },
  limits: DEFAULT_LIMITS,
  workouts: [summary()],
  ...over,
});

const detail = (over: Partial<WorkoutDetail> = {}): WorkoutDetail => ({
  id: 7,
  revision: 1,
  created_at: NOW,
  updated_at: NOW,
  workout: BODY,
  send: idle,
  ...over,
});

function Where() {
  const l = useLocation();
  return <div data-testid="where">{l.pathname}</div>;
}

function renderApp(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return { client, ...render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/workouts" element={<WorkoutListPage />} />
          <Route path="/workouts/new" element={<WorkoutPage />} />
          <Route path="/workouts/:id" element={<WorkoutPage />} />
          <Route path="/strength" element={<ActivitiesPage strength />} />
        </Routes>
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  ) };
}

const bodyOf = (init: RequestInit | undefined): unknown => JSON.parse(String(init?.body));

describe('the workout list', () => {
  it('shows each workout with its sport, steps, time and where it is on its way to the watch', async () => {
    mockApi({
      '/api/workouts': list({
        workouts: [
          summary({ id: 7, name: 'Tempo 20', send: queued }),
          summary({ id: 8, name: 'Long run', steps: 1, duration_s: null, send: failed }),
          summary({ id: 9, name: 'Hills', sport: 'cycling', steps: 6, duration_s: 3900, send: { state: 'sent', requested_at: NOW, finished_at: new Date(Date.now() - 2 * 3600_000).toISOString(), error: null } }),
        ],
      }),
    });
    renderApp('/workouts');
    const rows = await screen.findAllByRole('link', { name: /Tempo 20|Long run|Hills/ });
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveAttribute('href', '/workouts/7');
    expect(rows[0]).toHaveTextContent('Running · 2 steps · 30 min');
    expect(rows[0]).toHaveTextContent('Waiting for the phone');
    expect(rows[1]).toHaveTextContent('Running · 1 step');
    expect(rows[1]).not.toHaveTextContent('min');
    expect(rows[1]).toHaveTextContent('Could not be sent');
    expect(rows[1]).toHaveTextContent('Open it to see why');
    expect(rows[2]).toHaveTextContent('Cycling · 6 steps · 1 h 05 min');
    expect(rows[2]).toHaveTextContent('On the watch, 2 h ago');
    expect(screen.getByRole('heading', { level: 1, name: 'Workouts' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'New workout' })).toHaveAttribute('href', '/workouts/new');
  });

  it('says there are none yet and offers to build one', async () => {
    mockApi({ '/api/workouts': list({ workouts: [] }) });
    renderApp('/workouts');
    expect(await screen.findByText('No workouts yet')).toBeInTheDocument();
    expect(screen.getAllByRole('link', { name: 'New workout' }).length).toBeGreaterThan(0);
  });

  it('says so when this server cannot write workout files, and still lists the workouts', async () => {
    mockApi({ '/api/workouts': list({ watch_send: { available: false, reason: 'this server was built without the FIT profile' } }) });
    renderApp('/workouts');
    expect(await screen.findByText('this server was built without the FIT profile')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Tempo 20/ })).toBeInTheDocument();
  });

  it('shows an error with a way to retry', async () => {
    mockApi({ '/api/workouts': () => json({ error: 'database is locked' }, { status: 500 }) });
    renderApp('/workouts');
    expect(await screen.findByRole('alert')).toHaveTextContent('database is locked');
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
  });

  it('is reached from the strength list', async () => {
    mockApi({ '/api/activities': { sample: false, items: [], next_before: null } });
    renderApp('/strength');
    expect(await screen.findByRole('link', { name: 'Workouts' })).toHaveAttribute('href', '/workouts');
  });
});

describe('building a workout', () => {
  it('saves a new workout as the body the server stores, then opens it', async () => {
    const user = userEvent.setup();
    let saved: unknown = null;
    mockApi({
      'POST /api/workouts': (_url: string, init?: RequestInit) => {
        saved = bodyOf(init);
        return json(detail({ id: 9, workout: saved as WorkoutBody }), { status: 201 });
      },
      'GET /api/workouts/9': detail({ id: 9, workout: { name: 'Easy run', sport: 'running', nodes: [] } }),
      'GET /api/workouts': list({ workouts: [] }),
    });
    renderApp('/workouts/new');
    await user.type(await screen.findByLabelText('Name'), 'Easy run');
    await user.click(screen.getByRole('button', { name: 'Save workout' }));
    await waitFor(() => expect(screen.getByTestId('where')).toHaveTextContent('/workouts/9'));
    expect(saved).toEqual({ name: 'Easy run', sport: 'running', nodes: [{ type: 'step', intensity: 'warmup', duration: { kind: 'time', seconds: 600 }, target: { kind: 'none' } }] });
    expect(await screen.findByText('Workout saved.')).toBeInTheDocument();
    expect(screen.getByText(/Not sent to the watch yet/)).toBeInTheDocument();
  });

  it('does not save while something is wrong: it says what and puts focus on the first field to fix', async () => {
    const user = userEvent.setup();
    const { calls } = mockApi({ 'GET /api/workouts': list({ workouts: [] }) });
    renderApp('/workouts/new');
    await screen.findByLabelText('Name');
    await user.click(screen.getByRole('button', { name: 'Save workout' }));
    expect(await screen.findByText('Give the workout a name.')).toBeInTheDocument();
    expect(screen.getByLabelText('Name')).toHaveAttribute('aria-invalid', 'true');
    await waitFor(() => expect(screen.getByLabelText('Name')).toHaveFocus());
    expect(screen.getAllByRole('alert').some((a) => /some details need attention/.test(a.textContent ?? ''))).toBe(true);
    expect(calls.some((c) => c.init?.method === 'POST')).toBe(false);

    // Fixing it clears the message; a bad step time is named at the step.
    await user.type(screen.getByLabelText('Name'), 'Intervals');
    const first = screen.getByRole('group', { name: 'Step 1' });
    await user.clear(within(first).getByLabelText('Time'));
    await user.type(within(first).getByLabelText('Time'), 'soon');
    expect(within(first).getByText(/Enter a time like 5:00/)).toBeInTheDocument();
    expect(screen.queryByText('Give the workout a name.')).not.toBeInTheDocument();
  });

  it('adds a repeat block with its steps, moves and removes steps by name', async () => {
    const user = userEvent.setup();
    mockApi({ 'GET /api/workouts': list({ workouts: [] }) });
    renderApp('/workouts/new');
    await user.click(await screen.findByRole('button', { name: 'Add repeat block' }));
    await user.click(screen.getByRole('button', { name: 'Add step' }));

    const legends = () => screen.getAllByRole('group').map((g) => g.querySelector('legend')?.textContent).filter(Boolean);
    expect(legends()).toEqual(['Step 1', 'Repeat block 2', 'Step 2.1', 'Step 2.2', 'Step 3']);
    expect(screen.getByLabelText('Repeat times')).toHaveValue('4');
    expect(screen.getByText(/4 steps/)).toBeInTheDocument();

    // The last step moves above the block; the first cannot move up.
    expect(screen.getByRole('button', { name: 'Move step 1 up' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Move step 3 up' }));
    expect(legends()).toEqual(['Step 1', 'Step 2', 'Repeat block 3', 'Step 3.1', 'Step 3.2']);

    // A block keeps at least one step.
    await user.click(screen.getByRole('button', { name: 'Remove step 3.2' }));
    expect(screen.getByRole('button', { name: 'Remove step 3.1' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Remove repeat block 3' }));
    expect(legends()).toEqual(['Step 1', 'Step 2']);

    await user.click(screen.getByRole('button', { name: 'Copy step 2' }));
    expect(legends()).toEqual(['Step 1', 'Step 2', 'Step 3']);
  });

  it('stops adding at the limit of steps and says why', async () => {
    mockApi({ 'GET /api/workouts': list({ workouts: [], limits: { ...DEFAULT_LIMITS, max_steps: 3 } }) });
    const user = userEvent.setup();
    renderApp('/workouts/new');
    await user.click(await screen.findByRole('button', { name: 'Add step' }));
    await user.click(screen.getByRole('button', { name: 'Add step' }));
    expect(screen.getByRole('button', { name: 'Add step' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Add repeat block' })).toBeDisabled();
    expect(screen.getByText('The watch holds at most 3 steps in one workout.')).toBeInTheDocument();
  });

  it('shows the fields of the chosen target and ends', async () => {
    const user = userEvent.setup();
    mockApi({ 'GET /api/workouts': list({ workouts: [] }) });
    renderApp('/workouts/new');
    const step = within(await screen.findByRole('group', { name: 'Step 1' }));
    await user.selectOptions(step.getByLabelText('Target'), 'pace');
    expect(step.getByLabelText('Fastest pace (min/km)')).toBeInTheDocument();
    expect(step.getByLabelText('Slowest pace (min/km)')).toBeInTheDocument();
    await user.selectOptions(step.getByLabelText('Target'), 'heart_rate_zone');
    expect(step.getByLabelText('Zone')).toBeInTheDocument();
    expect(step.queryByLabelText('Fastest pace (min/km)')).not.toBeInTheDocument();
    await user.selectOptions(step.getByLabelText('Ends'), 'open');
    expect(step.queryByLabelText('Time')).not.toBeInTheDocument();
    await user.selectOptions(step.getByLabelText('Ends'), 'distance');
    expect(step.getByLabelText('Distance (m)')).toBeInTheDocument();
  });

  it('keeps what was typed and shows the server\'s reason when saving fails', async () => {
    const user = userEvent.setup();
    mockApi({
      'POST /api/workouts': () => json({ error: 'a user keeps at most 200 workouts; delete one first' }, { status: 409 }),
      'GET /api/workouts': list({ workouts: [] }),
    });
    renderApp('/workouts/new');
    await user.type(await screen.findByLabelText('Name'), 'Easy run');
    await user.click(screen.getByRole('button', { name: 'Save workout' }));
    expect(await screen.findByText(/a user keeps at most 200 workouts/)).toBeInTheDocument();
    expect(screen.getByLabelText('Name')).toHaveValue('Easy run');
    expect(screen.getByTestId('where')).toHaveTextContent('/workouts/new');
  });
});

describe('a saved workout', () => {
  it('opens with its values and saves a change with a PUT', async () => {
    const user = userEvent.setup();
    let put: unknown = null;
    mockApi({
      'PUT /api/workouts/7': (_url: string, init?: RequestInit) => {
        put = bodyOf(init);
        return json(detail({ revision: 2, workout: put as WorkoutBody }));
      },
      'GET /api/workouts/7': detail(),
      'GET /api/workouts': list(),
    });
    renderApp('/workouts/7');
    const name = await screen.findByLabelText('Name');
    expect(name).toHaveValue('Tempo 20');
    expect(screen.getByRole('heading', { level: 1, name: 'Tempo 20' })).toBeInTheDocument();
    const second = within(screen.getByRole('group', { name: 'Step 2' }));
    expect(second.getByLabelText('Time')).toHaveValue('20:00');
    expect(second.getByLabelText('Zone')).toHaveValue('3');

    await user.clear(name);
    await user.type(name, 'Tempo 25');
    await user.clear(second.getByLabelText('Time'));
    await user.type(second.getByLabelText('Time'), '25:00');
    await user.click(screen.getByRole('button', { name: 'Save workout' }));
    expect(await screen.findByText('Workout saved.')).toBeInTheDocument();
    expect(put).toEqual({
      name: 'Tempo 25',
      sport: 'running',
      nodes: [
        { type: 'step', intensity: 'warmup', duration: { kind: 'time', seconds: 600 }, target: { kind: 'none' } },
        { type: 'step', intensity: 'active', duration: { kind: 'time', seconds: 1500 }, target: { kind: 'heart_rate_zone', zone: 3 } },
      ],
    });
  });

  it('sends it to the watch, shows it waiting for the phone, and takes it back', async () => {
    const user = userEvent.setup();
    let state: SendInfo = idle;
    const { calls } = mockApi({
      'POST /api/workouts/7/send': () => {
        state = queued;
        return json(detail({ send: state }));
      },
      'DELETE /api/workouts/7/send': () => {
        state = idle;
        return json(detail({ send: state }));
      },
      'GET /api/workouts/7': () => json(detail({ send: state })),
      'GET /api/workouts': () => json(list({ workouts: [summary({ send: state })] })),
    });
    renderApp('/workouts/7');
    expect(await screen.findByText('Not sent to the watch yet.')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Send to watch' }));
    expect(await screen.findByText(/Waiting for the phone/)).toBeInTheDocument();
    expect(calls.filter((c) => c.init?.method === 'POST').map((c) => c.url)).toEqual(['/api/workouts/7/send']);
    expect(await screen.findByText('It goes to the watch when the phone is connected to it.')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Cancel sending' }));
    expect(await screen.findByText('Not sent to the watch yet.')).toBeInTheDocument();
    expect(await screen.findByText('Sending cancelled.')).toBeInTheDocument();
  });

  it('shows a version saved elsewhere, unless it has unsaved changes of its own', async () => {
    const user = userEvent.setup();
    let saved = detail();
    mockApi({ 'GET /api/workouts/7': () => json(saved), 'GET /api/workouts': list() });
    const { client } = renderApp('/workouts/7');
    const name = await screen.findByLabelText('Name');
    expect(name).toHaveValue('Tempo 20');

    // Renamed on the web (or the server's state reached the phone): the untouched form follows.
    saved = detail({ revision: 2, workout: { ...BODY, name: 'Tempo 30' } });
    await client.invalidateQueries();
    await waitFor(() => expect(screen.getByLabelText('Name')).toHaveValue('Tempo 30'));
    expect(screen.queryByText(/You have unsaved changes/)).not.toBeInTheDocument();

    // With a change typed here, the typed text stays.
    await user.type(screen.getByLabelText('Name'), '!');
    saved = detail({ revision: 3, workout: { ...BODY, name: 'Tempo 40' } });
    await client.invalidateQueries();
    await waitFor(() => expect(screen.getByRole('heading', { level: 1, name: 'Tempo 40' })).toBeInTheDocument());
    expect(screen.getByLabelText('Name')).toHaveValue('Tempo 30!');
  });

  it('sends the saved version: with unsaved changes the button waits', async () => {
    const user = userEvent.setup();
    mockApi({ 'GET /api/workouts/7': detail(), 'GET /api/workouts': list() });
    renderApp('/workouts/7');
    const send = await screen.findByRole('button', { name: 'Send to watch' });
    expect(send).toBeEnabled();
    await user.type(screen.getByLabelText('Name'), '!');
    expect(send).toBeDisabled();
    expect(screen.getByText(/You have unsaved changes. Save first/)).toBeInTheDocument();
    expect(send).toHaveAccessibleDescription(/Save first/);
  });

  it('saves and sends in one go', async () => {
    const user = userEvent.setup();
    const { calls } = mockApi({
      'PUT /api/workouts/7': (_url: string, init?: RequestInit) => json(detail({ revision: 2, workout: bodyOf(init) as WorkoutBody })),
      'POST /api/workouts/7/send': () => json(detail({ revision: 2, send: queued })),
      'GET /api/workouts/7': detail(),
      'GET /api/workouts': list(),
    });
    renderApp('/workouts/7');
    await user.click(await screen.findByRole('button', { name: 'Save and send to watch' }));
    expect(await screen.findByText('Workout saved. It goes to the watch when the phone is connected to it.')).toBeInTheDocument();
    const writes = calls.filter((c) => c.init?.method && c.init.method !== 'GET').map((c) => `${c.init?.method} ${c.url}`);
    expect(writes).toEqual(['PUT /api/workouts/7', 'POST /api/workouts/7/send']);
  });

  it('keeps the saved workout when it could not be queued, and says so', async () => {
    const user = userEvent.setup();
    mockApi({
      'PUT /api/workouts/7': (_url: string, init?: RequestInit) => json(detail({ revision: 2, workout: bodyOf(init) as WorkoutBody })),
      'POST /api/workouts/7/send': () => json({ error: 'this server was built without the FIT profile' }, { status: 503 }),
      'GET /api/workouts/7': detail(),
      'GET /api/workouts': list(),
    });
    renderApp('/workouts/7');
    await user.click(await screen.findByRole('button', { name: 'Save and send to watch' }));
    expect(await screen.findByText(/Workout saved, but it could not be sent to the watch. this server was built without the FIT profile/)).toBeInTheDocument();
  });

  it('shows why the watch did not take it, and offers to try again', async () => {
    mockApi({ 'GET /api/workouts/7': detail({ send: failed }), 'GET /api/workouts': list() });
    renderApp('/workouts/7');
    expect(await screen.findByText(/The watch did not take it: The watch refused the workout while creating the file \(status 2, NO_SPACE\)/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
  });

  it('offers to send it again once it is on the watch', async () => {
    mockApi({
      'GET /api/workouts/7': detail({ send: { state: 'sent', requested_at: NOW, finished_at: new Date(Date.now() - 5 * 60_000).toISOString(), error: null } }),
      'GET /api/workouts': list(),
    });
    renderApp('/workouts/7');
    expect(await screen.findByText(/Sent to the watch 5 min ago/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Send again' })).toBeInTheDocument();
  });

  it('can be saved but not sent when the server has no FIT profile, and says why', async () => {
    mockApi({
      'GET /api/workouts/7': detail(),
      'GET /api/workouts': list({ watch_send: { available: false, reason: 'this server was built without the FIT profile, so it cannot write workout files for the watch; workouts can still be saved here' } }),
    });
    renderApp('/workouts/7');
    expect(await screen.findByText(/built without the FIT profile/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Send to watch|Save and send/ })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save workout' })).toBeEnabled();
  });

  it('asks before deleting and goes back to the list', async () => {
    const user = userEvent.setup();
    const { calls } = mockApi({
      'DELETE /api/workouts/7': () => new Response(null, { status: 204 }),
      'GET /api/workouts/7': detail(),
      'GET /api/workouts': list({ workouts: [] }),
    });
    renderApp('/workouts/7');
    await user.click(await screen.findByRole('button', { name: 'Delete workout' }));
    const dialog = await screen.findByRole('dialog', { name: 'Delete this workout?' });
    expect(calls.some((c) => c.init?.method === 'DELETE')).toBe(false);
    await user.click(within(dialog).getByRole('button', { name: 'Delete workout' }));
    await waitFor(() => expect(screen.getByTestId('where')).toHaveTextContent(/^\/workouts$/));
    expect(await screen.findByText('No workouts yet')).toBeInTheDocument();
  });

  it('says when the workout does not exist', async () => {
    mockApi({ 'GET /api/workouts/99': () => json({ error: 'no such workout' }, { status: 404 }), 'GET /api/workouts': list() });
    renderApp('/workouts/99');
    expect(await screen.findByText('This workout was not found')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Go to workouts' })).toHaveAttribute('href', '/workouts');
  });

  it('does not treat a made-up address as a workout', async () => {
    const { calls } = mockApi({ 'GET /api/workouts': list() });
    renderApp('/workouts/abc');
    expect(await screen.findByText('This workout was not found')).toBeInTheDocument();
    expect(calls.some((c) => c.url.includes('/api/workouts/abc'))).toBe(false);
  });
});
