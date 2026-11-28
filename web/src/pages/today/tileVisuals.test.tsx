import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { HomeCard, Today } from '../../api/generated';
import { GlanceTile } from './tiles';
import {
  GAUGE_SWEEP,
  GAUGE_START,
  READINESS_EDGES,
  arcPath,
  dialArcs,
  gaugeAngle,
  hrBands,
  hrvDots,
  hrvScale,
  latestValue,
  minuteOfDay,
  polar,
  readinessSegments,
  recoveryLeft,
  stageBlocks,
  stressBuckets,
  stressSegments,
  stressSlices,
} from './tileVisuals';
import { makeEmptyToday, makeToday } from './testData';

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

function stubApi(routes: Record<string, unknown> = {}) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      for (const [prefix, body] of Object.entries(routes)) if (url.startsWith(prefix)) return json(body);
      return json({ error: 'no' }, 404);
    }),
  );
}

function renderTile(id: HomeCard, today: Today) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <GlanceTile id={id} today={today} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.unstubAllGlobals());

describe('gauge geometry', () => {
  it('puts the marker at the start, the middle and the end of the 270 degree arc', () => {
    expect(GAUGE_SWEEP).toBe(270);
    expect(gaugeAngle(0)).toBe(-135);
    expect(gaugeAngle(0.5)).toBe(0);
    expect(gaugeAngle(1)).toBe(135);
    expect(gaugeAngle(7)).toBe(135); // held to the end
    expect(gaugeAngle(-3)).toBe(-135);
    expect(gaugeAngle(Number.NaN)).toBe(-135);
  });

  it('turns an angle into a point: 0 degrees is straight up, 90 is to the right', () => {
    const top = polar(60, 60, 40, 0);
    expect(top.x).toBeCloseTo(60);
    expect(top.y).toBeCloseTo(20);
    const right = polar(60, 60, 40, 90);
    expect(right.x).toBeCloseTo(100);
    expect(right.y).toBeCloseTo(60);
    const middle = polar(60, 60, 40, gaugeAngle(0.5));
    expect(middle.x).toBeCloseTo(60);
  });

  it('uses the large-arc flag only for arcs wider than half a circle', () => {
    expect(arcPath(60, 60, 40, -135, 0)).toMatch(/ 0 0 1 /);
    expect(arcPath(60, 60, 40, -135, 135)).toMatch(/ 0 1 1 /);
  });

  it('leaves a gap between neighbouring segments and none at the open ends of a gauge', () => {
    const arcs = dialArcs(
      [
        { key: 'a', from: 0, to: 0.5, color: 'x' },
        { key: 'b', from: 0.5, to: 1, color: 'y' },
      ],
      GAUGE_START,
      GAUGE_SWEEP,
      4,
      false,
    );
    expect(arcs[0]!.from).toBe(-135);
    expect(arcs[1]!.to).toBe(135);
    expect(arcs[1]!.from - arcs[0]!.to).toBeCloseTo(4);
  });

  it('skips empty segments', () => {
    expect(dialArcs([{ key: 'a', from: 0.2, to: 0.2, color: 'x' }], 0, 360, 3, true)).toEqual([]);
  });
});

describe('heart rate', () => {
  it('splits the day\'s range evenly into five bands, and gives none for a flat day', () => {
    const bands = hrBands(50, 150)!;
    expect(bands.map((b) => b.zone)).toEqual([1, 2, 3, 4, 5]);
    expect(bands[0]).toEqual({ zone: 1, from: 50, to: 70 });
    expect(bands[4]).toEqual({ zone: 5, from: 130, to: 150 });
    expect(hrBands(80, 80)).toBeNull();
    expect(hrBands(Number.NaN, 80)).toBeNull();
  });

  it('takes the newest reading that has a value', () => {
    expect(latestValue([{ value: 60 }, { value: 72 }, { value: null }])).toBe(72);
    expect(latestValue([{ value: null }])).toBeNull();
    expect(latestValue([])).toBeNull();
  });
});

describe('stress', () => {
  it('splits the ring by the time at each level, in order, leaving out levels without time', () => {
    const slices = stressSlices({ rest_s: 3600, low_s: 0, medium_s: 1200, high_s: null });
    expect(slices.map((s) => s.level)).toEqual(['rest', 'medium']);
    const segs = stressSegments(slices);
    expect(segs[0]).toMatchObject({ key: 'rest', from: 0 });
    expect(segs[0]!.to).toBeCloseTo(0.75);
    expect(segs[1]!.from).toBeCloseTo(0.75);
    expect(segs[1]!.to).toBeCloseTo(1);
    expect(stressSegments([])).toEqual([]);
  });

  it('reads the minute of the day in the owner\'s zone', () => {
    expect(minuteOfDay('2026-11-19T10:30:00+00:00', 'Etc/UTC')).toBe(630);
    expect(minuteOfDay('2026-11-19T23:30:00+00:00', 'Europe/Copenhagen')).toBe(30);
    expect(minuteOfDay('nope', 'Etc/UTC')).toBeNull();
  });

  it('puts readings in 5-minute buckets from midnight and colours each bucket by its main level', () => {
    const buckets = stressBuckets(
      [
        { time: '2026-11-19T00:01:00+00:00', stress: 10, level: 'rest' },
        { time: '2026-11-19T00:03:00+00:00', stress: 12, level: 'rest' },
        { time: '2026-11-19T00:04:00+00:00', stress: 60, level: 'medium' },
        { time: '2026-11-19T00:07:00+00:00', stress: null, level: 'activity' },
        { time: '2026-11-19T23:58:00+00:00', stress: 90, level: 'high' },
      ],
      'Etc/UTC',
    );
    expect(buckets).toHaveLength(288);
    expect(buckets[0]).toBe('rest');
    expect(buckets[1]).toBe('activity');
    expect(buckets[2]).toBeNull();
    expect(buckets[287]).toBe('high');
  });
});

describe('sleep stages', () => {
  const stages = [
    { start: '2026-11-18T23:00:00+00:00', end: '2026-11-19T01:00:00+00:00', stage: 'light' as const },
    { start: '2026-11-19T01:00:00+00:00', end: '2026-11-19T03:00:00+00:00', stage: 'deep' as const },
    { start: '2026-11-19T03:00:00+00:00', end: '2026-11-19T04:00:00+00:00', stage: 'unmeasurable' as const },
    { start: '2026-11-19T04:00:00+00:00', end: '2026-11-19T09:00:00+00:00', stage: 'rem' as const },
  ];

  it('gives each stage its start and width as a share of the night, cuts off what is outside it, and skips unmeasured stretches', () => {
    const blocks = stageBlocks(stages, '2026-11-18T23:00:00+00:00', '2026-11-19T07:00:00+00:00');
    expect(blocks.map((b) => b.stage)).toEqual(['light', 'deep', 'rem']);
    expect(blocks[0]).toEqual({ stage: 'light', x: 0, w: 0.25 });
    expect(blocks[1]!.x).toBeCloseTo(0.25);
    expect(blocks[2]!.x + blocks[2]!.w).toBeCloseTo(1);
  });

  it('gives nothing for a night without length', () => {
    expect(stageBlocks(stages, '2026-11-19T07:00:00+00:00', '2026-11-19T07:00:00+00:00')).toEqual([]);
  });
});

describe('HRV', () => {
  it('draws low, unbalanced, balanced and unbalanced zones, with the balanced one on the watch\'s own range', () => {
    const scale = hrvScale(55, 70)!;
    expect(scale.zones.map((z) => z.key)).toEqual(['low', 'unbalanced-low', 'balanced', 'unbalanced-high']);
    expect(scale.zones[0]!.from).toBe(0);
    expect(scale.zones[3]!.to).toBe(1);
    const balanced = scale.zones[2]!;
    expect(scale.at(55)).toBeCloseTo(balanced.from);
    expect(scale.at(70)).toBeCloseTo(balanced.to);
    for (let i = 1; i < 4; i += 1) expect(scale.zones[i]!.from).toBeCloseTo(scale.zones[i - 1]!.to);
  });

  it('holds the marker to the ends of the bar and gives no scale for an empty range', () => {
    const scale = hrvScale(55, 70)!;
    expect(scale.at(0)).toBe(0);
    expect(scale.at(500)).toBe(1);
    expect(hrvScale(60, 60)).toBeNull();
  });

  it('lists 28 nights ending today, oldest first, a night without a status as null', () => {
    const dots = hrvDots([{ date: '2026-11-19', status: 'balanced' }, { date: '2026-11-18', status: null }], '2026-11-19');
    expect(dots).toHaveLength(28);
    expect(dots[0]!.date).toBe('2026-10-23');
    expect(dots[27]).toEqual({ date: '2026-11-19', status: 'balanced' });
    expect(dots[26]).toEqual({ date: '2026-11-18', status: null });
    expect(dots[10]!.status).toBeNull();
  });
});

describe('readiness', () => {
  it('splits the gauge into the five levels at 25, 50, 75 and 95', () => {
    expect([...READINESS_EDGES]).toEqual([0, 25, 50, 75, 95, 100]);
    const segs = readinessSegments({ poor: 'a', low: 'b', moderate: 'c', high: 'd', prime: 'e' });
    expect(segs.map((s) => s.key)).toEqual(['poor', 'low', 'moderate', 'high', 'prime']);
    expect(segs[0]!.from).toBe(0);
    expect(segs[2]!.to).toBe(0.75);
    expect(segs[4]!.to).toBe(1);
  });

  it('puts the marker of a score on its level\'s segment: 25 starts Low, 94 is still High', () => {
    const at = (score: number) => gaugeAngle(score / 100);
    const segs = dialArcs(readinessSegments({ poor: 'a', low: 'b', moderate: 'c', high: 'd', prime: 'e' }), GAUGE_START, GAUGE_SWEEP, 0, false);
    const inside = (key: string, angle: number) => {
      const a = segs.find((s) => s.key === key)!;
      return angle >= a.from - 1e-9 && angle <= a.to + 1e-9;
    };
    expect(inside('poor', at(0))).toBe(true);
    expect(inside('low', at(25))).toBe(true);
    expect(inside('high', at(94))).toBe(true);
    expect(inside('prime', at(100))).toBe(true);
  });
});

describe('recovery', () => {
  it('is the time left over the whole time, or what the percent says', () => {
    const r = makeToday().recovery_time;
    expect(recoveryLeft({ ...r, total_s: 1000, remaining_s: 250 })).toBe(0.25);
    expect(recoveryLeft({ ...r, total_s: null, remaining_s: 250, percent_complete: 40 })).toBeCloseTo(0.6);
    expect(recoveryLeft({ ...r, remaining_s: null })).toBeNull();
    expect(recoveryLeft({ ...r, total_s: 100, remaining_s: 300 })).toBe(1);
  });
});

describe('tiles', () => {
  const day = '2026-11-19';
  const iso = (m: number) => new Date(Date.parse(`${day}T00:00:00Z`) + m * 60000).toISOString().replace('.000Z', '+00:00');

  it('heart rate: a gauge with the latest reading, and the resting rate as the figure', () => {
    const today = makeToday({ heart_rate: { resting_hr_bpm: 51, avg_7d_resting_hr_bpm: 52, day: { low_bpm: 48, high_bpm: 148, series: [{ time: iso(0), value: 60 }, { time: iso(60), value: 97 }] } } });
    renderTile('heart_rate', today);
    expect(screen.getByRole('img', { name: /Heart rate now 97 bpm\. Today's range is 48 to 148 bpm/ })).toBeInTheDocument();
    expect(screen.getByText('51 bpm')).toBeInTheDocument();
    expect(screen.getByText('Resting')).toBeInTheDocument();
  });

  it('empty tiles say so in a sentence, never read "unavailable" or 0, and keep the same structure', () => {
    stubApi();
    const empty = makeEmptyToday();
    for (const [id, text] of [
      ['heart_rate', 'No heart rate readings yet today.'],
      ['stress', 'No stress readings yet today.'],
      ['sleep', 'The watch recorded no sleep for last night.'],
      ['intensity_minutes', 'No intensity minutes this week.'],
      ['training_load', 'No training load from the watch this week.'],
      ['recovery_time', 'No recovery time from the watch yet.'],
      ['training_status', 'No training status yet.'],
    ] as [HomeCard, string][]) {
      const { container, unmount } = renderTile(id, empty);
      expect(screen.getByText(text)).toBeInTheDocument();
      expect(container.querySelector('.tile__visual')).not.toBeNull();
      expect(container.querySelector('.tile__figure')).not.toBeNull();
      expect(container.textContent).not.toMatch(/(^|\s)0(\s|$)/);
      expect(container.textContent).not.toMatch(/unavailable/);
      unmount();
    }
  });

  it('sleep: a night with a time but no score shows only the time asleep', () => {
    const t = makeToday();
    renderTile('sleep', makeToday({ sleep: { ...t.sleep!, score: null } }));
    expect(screen.getByText('7h 12m')).toBeInTheDocument();
    expect(screen.queryByText('Score')).not.toBeInTheDocument();
    expect(screen.queryByText('unavailable')).not.toBeInTheDocument();
  });

  it('training load: no figure and no label when the week has no load, a sentence instead', () => {
    const t = makeToday();
    renderTile('training_load', makeToday({ training_load: { ...t.training_load, seven_day_load: null } }));
    expect(screen.getByText('No training load from the watch this week.')).toBeInTheDocument();
    expect(screen.queryByText(/7 days/)).not.toBeInTheDocument();
    expect(screen.queryByText('unavailable')).not.toBeInTheDocument();
  });

  it('stress: the level word is written next to its dot', () => {
    renderTile('stress', makeToday({ stress: { ...makeToday().stress, main_level: 'low', average: 31 } }));
    expect(screen.getByText('Low')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: /Average stress 31 out of 100\. Time today: rest 1h 0m/ })).toBeInTheDocument();
  });

  it('sleep: score and time asleep side by side, stage times in the picture\'s sentence', () => {
    renderTile('sleep', makeToday());
    expect(screen.getByText('81')).toBeInTheDocument();
    expect(screen.getByText('7h 12m')).toBeInTheDocument();
    expect(screen.getByText('Good')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: /^Sleep stages: deep 1h 40m, light 3h 20m, REM 1h 57m, awake 10m\. From 23:30 to 06:36\.$/ })).toBeInTheDocument();
  });

  it('training status: the status word and the date it started', () => {
    renderTile('training_status', makeToday({ training_status: { ...makeToday().training_status, label: 'Productive', kind: 'productive' } }));
    expect(screen.getByText('Productive')).toBeInTheDocument();
    expect(screen.getByText('Since Sun 1 Nov')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Training status Productive' })).toBeInTheDocument();
  });

  it('intensity minutes: the minutes in the ring against the goal', () => {
    renderTile('intensity_minutes', makeToday());
    expect(screen.getByRole('img', { name: 'Intensity minutes this week: 70 of the 150 minute goal' })).toBeInTheDocument();
    expect(screen.getByText('80 min')).toBeInTheDocument();
  });

  it('training load: the 7-day load and its target range', () => {
    renderTile('training_load', makeToday());
    expect(screen.getByRole('img', { name: '7-day load 306. Target range 230 to 403.' })).toBeInTheDocument();
    expect(screen.getByText('In range · 7 days')).toBeInTheDocument();
  });

  it('recovery: the time left in the ring and the time it is over', () => {
    renderTile('recovery_time', makeToday());
    expect(screen.getByRole('img', { name: 'Recovery time left 7h 28m' })).toBeInTheDocument();
    expect(screen.getByText('05:43')).toBeInTheDocument();
    expect(screen.getByText('Ready around')).toBeInTheDocument();
  });

  it('readiness: the gauge, the level word, the headline and "Nimrag estimate"', async () => {
    stubApi({ '/api/readiness': { sample: true, date: day, score: 78, level: 'high', headline_key: 'high', headline: 'Ready for a solid session', sentence: null, factors: [], present: 6, data_days: 14, unavailable: null } });
    renderTile('training_readiness', makeToday());
    expect(await screen.findByRole('img', { name: 'Readiness estimate 78 out of 100, level High.' })).toBeInTheDocument();
    expect(screen.getByText('High')).toBeInTheDocument();
    expect(screen.getByText('Ready for a solid session')).toBeInTheDocument();
    expect(screen.getByText('Nimrag estimate')).toBeInTheDocument();
  });

  it('readiness without an estimate stays as an empty tile with the reason', async () => {
    stubApi({ '/api/readiness': { sample: true, date: day, score: null, level: null, headline_key: null, headline: null, sentence: null, factors: [], present: 2, data_days: 2, unavailable: 'Readiness needs at least 3 days of data.' } });
    const { container } = renderTile('training_readiness', makeToday());
    expect(await screen.findByText('Readiness needs at least 3 days of data.')).toBeInTheDocument();
    expect(container.querySelector('[data-card="training_readiness"]')).not.toBeNull();
  });

  it('HRV: status, 7-night average, balanced range and the last 4 weeks', async () => {
    const days = Array.from({ length: 28 }, (_, i) => ({ date: `2026-${i < 9 ? '10' : '11'}-${String(i < 9 ? 23 + i : i - 8).padStart(2, '0')}`, status: i % 7 === 0 ? 'unbalanced' : i < 5 ? null : 'balanced' }));
    stubApi({ '/api/hrv': { sample: true, from: '2026-10-23', to: day, today: day, days, nights: 20, overnight_avg_ms: 62, baseline_low_ms: 55, baseline_high_ms: 70, night: null } });
    const { container } = renderTile('hrv', makeToday());
    expect(screen.getByText('64 ms')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole('img', { name: /Last 4 weeks: / })).toBeInTheDocument());
    expect(screen.getByRole('img', { name: /Status Balanced\. 7-night average 64 ms\. Balanced range 55 to 70 ms\./ })).toBeInTheDocument();
    expect(container.querySelectorAll('.tv-dots__dot')).toHaveLength(28);
    expect(container.querySelectorAll('.tv-dots__dot--none').length).toBeGreaterThan(0);
  });

  it('steps: the number, the goal, the week of bars and the goal bar', async () => {
    stubApi({ '/api/metrics/steps': { sample: true, metric: 'steps', range: '7d', from: '2026-11-13', to: day, bucket: 'day', series: [{ key: 'steps', label: 'Steps', unit: 'steps', points: [9000, 4000, null, 8000, 8100, 100, 6240].map((value, i) => ({ date: `2026-11-${13 + i}`, value })) }], stats: [], goal: 8000, target_low: null, target_high: null, has_prev: true, has_next: false } });
    const { container } = renderTile('steps', makeToday());
    expect(screen.getByText('6,240')).toBeInTheDocument();
    expect(screen.getByText('Goal 8,000')).toBeInTheDocument();
    await waitFor(() => expect(container.querySelectorAll('.tv-bars__letters span')).toHaveLength(7));
    expect(screen.getByRole('img', { name: /^Steps over the last 7 days: .* Thu 19 Nov 6,240\./ })).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: '6,240 of 8,000 steps' })).toBeInTheDocument();
  });

  it('weight: the latest value and the change with its arrow', () => {
    renderTile('weight', makeToday());
    expect(screen.getByText('77.6 kg')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Change in the last 30 days: −0.5 kg' })).toBeInTheDocument();
  });

  it('every tile is one link to its page', () => {
    renderTile('vo2_max', makeToday());
    expect(document.querySelector('a[data-card="vo2_max"]')).toHaveAttribute('href', '/metrics/vo2_max');
  });
});
