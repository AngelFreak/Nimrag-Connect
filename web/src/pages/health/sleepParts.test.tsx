import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { NightSeries } from '../../api/generated/NightSeries';
import type { SleepNight } from '../../api/generated/SleepNight';
import { sleepFactors } from './sleepNight';
import { FactorsList, NightHeadline, OvernightStrip, overnightTiles, ScoreSummary, signed, StagesWithOverlay } from './sleepParts';

const NONE = { overall: null, quality: null, duration: null, recovery: null, deep: null, light: null, rem: null };

function night(over: Partial<SleepNight> = {}): SleepNight {
  return {
    date: '2026-11-19',
    start: '2026-11-18T23:00:00+01:00',
    end: '2026-11-19T07:00:00+01:00',
    duration_s: 7 * 3600 + 30 * 60,
    time_in_bed_s: 8 * 3600,
    score: 84,
    score_label: 'good',
    scores: NONE,
    awakenings: 2,
    avg_stress: 18,
    stages_available: true,
    deep_s: 5400,
    light_s: 14400,
    rem_s: 7200,
    awake_s: 1500,
    stages: [
      { start: '2026-11-18T23:00:00+01:00', end: '2026-11-19T01:00:00+01:00', stage: 'light' },
      { start: '2026-11-19T01:00:00+01:00', end: '2026-11-19T03:00:00+01:00', stage: 'deep' },
      { start: '2026-11-19T03:00:00+01:00', end: '2026-11-19T07:00:00+01:00', stage: 'rem' },
    ],
    hrv_ms: 61,
    lowest_hr_bpm: 47,
    breathing_brpm: 13.4,
    spo2_avg_percent: 95.4,
    spo2_low_percent: 91,
    sleep_need_min: 480,
    restless_moments: null,
    avg_hr_bpm: 54.5,
    energy_change: 50,
    breathing_low_brpm: 11.2,
    hrv_status: 'balanced',
    ...over,
  };
}

const points = (values: number[]) => values.map((value, i) => ({ minute: i * 60, value }));
const OVERLAYS: NightSeries[] = [
  { key: 'heart_rate', points: points([60, 52, 50, 54, 58]) },
  { key: 'energy', points: points([30, 45, 60, 72, 80]) },
  { key: 'spo2', points: [{ minute: 0, value: 95 }] }, // one reading is no line
];

describe('score summary', () => {
  it('shows the ring with its level word, time asleep, bed and wake time', () => {
    render(<ScoreSummary night={night()} />);
    expect(screen.getByRole('img', { name: 'Sleep score 84 out of 100' })).toBeInTheDocument();
    expect(screen.getByText('Good')).toBeInTheDocument();
    expect(screen.getByText('7h 30m')).toBeInTheDocument();
    expect(screen.getByText('23:00')).toBeInTheDocument();
    expect(screen.getByText('07:00')).toBeInTheDocument();
  });
  it('says unavailable without a score and shows no level word', () => {
    render(<ScoreSummary night={night({ score: null, score_label: null })} />);
    expect(screen.getByRole('img', { name: 'Sleep score unavailable' })).toBeInTheDocument();
    expect(screen.queryByText('Good')).not.toBeInTheDocument();
  });
});

describe('headline', () => {
  it('shows the title and one sentence', () => {
    const n = night({ duration_s: 5 * 3600 });
    render(<NightHeadline night={n} factors={sleepFactors(n)} />);
    expect(screen.getByText('Short night')).toBeInTheDocument();
    expect(screen.getByText(/3 h less than your 8 h need/)).toBeInTheDocument();
  });
});

describe('factor list', () => {
  it('has a row per factor with its value and rating word, and the target band behind a button', async () => {
    const user = userEvent.setup();
    render(<FactorsList factors={sleepFactors(night())} />);
    const list = screen.getByRole('list', { name: 'Sleep score factors' });
    expect(within(list).getAllByRole('listitem')).toHaveLength(6);
    const deep = within(list).getByRole('button', { name: /Deep sleep/ });
    expect(deep).toHaveTextContent('1h 30m · 20.0 %');
    expect(deep).toHaveTextContent('Excellent');
    expect(deep).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getByText(/Target 16 to 33 %/)).not.toBeVisible();
    await user.click(deep);
    expect(deep).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText(/Target 16 to 33 %/)).toBeVisible();
    await user.click(deep);
    expect(screen.getByText(/Target 16 to 33 %/)).not.toBeVisible();
  });

  it('explains the watch score when the watch rated the factor', async () => {
    const user = userEvent.setup();
    render(<FactorsList factors={sleepFactors(night({ scores: { ...NONE, duration: 75 } }))} />);
    const row = screen.getByRole('button', { name: /Duration/ });
    expect(row).toHaveTextContent('Fair');
    await user.click(row);
    expect(screen.getByText(/Your watch scored this 75 of 100; Good is 80 or more/)).toBeVisible();
    expect(screen.queryByText(/Within 30 min/)).toBeNull();
  });

  it('leaves out the stress row when there is no stress value', () => {
    render(<FactorsList factors={sleepFactors(night({ avg_stress: null }))} />);
    expect(screen.queryByRole('button', { name: /Stress/ })).not.toBeInTheDocument();
    expect(screen.getAllByRole('listitem')).toHaveLength(5);
  });
});

describe('overnight strip', () => {
  it('shows a tile for each measurement the night has', () => {
    render(<OvernightStrip night={night({ restless_moments: 9 })} />);
    const strip = within(screen.getByRole('list', { name: 'Overnight measurements' }));
    expect(strip.getByText('Lowest heart rate').closest('.card')).toHaveTextContent('47 bpm');
    expect(strip.getByText('Average heart rate').closest('.card')).toHaveTextContent('55 bpm');
    expect(strip.getByText('Energy change').closest('.card')).toHaveTextContent('+50');
    expect(strip.getByText('Blood oxygen').closest('.card')).toHaveTextContent('95.4 %');
    expect(strip.getByText('Blood oxygen').closest('.card')).toHaveTextContent('Lowest 91 %');
    expect(strip.getByText('Breathing rate').closest('.card')).toHaveTextContent('13.4 /min');
    expect(strip.getByText('Breathing rate').closest('.card')).toHaveTextContent('Lowest 11.2 /min');
    expect(strip.getByText('Overnight HRV').closest('.card')).toHaveTextContent('61 ms');
    expect(strip.getByText('7-day HRV status').closest('.card')).toHaveTextContent('Balanced');
    expect(strip.getByText('Awakenings').closest('.card')).toHaveTextContent('2');
    expect(strip.getByText('Restless moments').closest('.card')).toHaveTextContent('9');
    expect(strip.getByText('Average sleep stress').closest('.card')).toHaveTextContent('18');
    expect(strip.getByText('Time in bed').closest('.card')).toHaveTextContent('8h 0m');
  });

  it('hides every tile that has no data', () => {
    const bare = night({
      hrv_ms: null,
      lowest_hr_bpm: null,
      breathing_brpm: null,
      spo2_avg_percent: null,
      spo2_low_percent: null,
      avg_hr_bpm: null,
      energy_change: null,
      breathing_low_brpm: null,
      hrv_status: null,
      awakenings: null,
      avg_stress: null,
    });
    expect(overnightTiles(bare).map((t) => t.key)).toEqual(['in_bed']);
    render(<OvernightStrip night={bare} />);
    expect(screen.queryByText('Overnight HRV')).not.toBeInTheDocument();
    expect(screen.queryByText('Blood oxygen')).not.toBeInTheDocument();
    expect(screen.queryByText('Restless moments')).not.toBeInTheDocument();
    expect(screen.queryByText('unavailable')).not.toBeInTheDocument();
  });

  it('writes energy change with a sign and a real minus', () => {
    expect([50, 3.4, 0, -12, -0.2].map(signed)).toEqual(['+50', '+3', '0', '−12', '0']);
  });

  it('does not show a status word it does not know', () => {
    expect(overnightTiles(night({ hrv_status: 'whatever' })).some((t) => t.key === 'hrv_status')).toBe(false);
  });
});

describe('overlay chips on the stage chart', () => {
  it('offers a chip only for series with a line to draw, none picked at first', () => {
    render(<StagesWithOverlay night={night()} overlays={OVERLAYS} />);
    const group = screen.getByRole('group', { name: 'Draw over the stages' });
    expect(within(group).getAllByRole('button').map((b) => b.textContent)).toEqual(['Heart rate', 'Energy']);
    within(group)
      .getAllByRole('button')
      .forEach((b) => expect(b).toHaveAttribute('aria-pressed', 'false'));
    expect(document.querySelector('.plot__y--right')).toBeNull();
  });

  it('draws one line at a time with its own axis, and switches it off again', async () => {
    const user = userEvent.setup();
    render(<StagesWithOverlay night={night()} overlays={OVERLAYS} />);
    await user.click(screen.getByRole('button', { name: 'Heart rate' }));
    expect(screen.getByRole('button', { name: 'Heart rate' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('img', { name: 'Sleep stages through the night, with heart rate over the night' })).toBeInTheDocument();
    expect(document.querySelectorAll('.plot__area svg path')).toHaveLength(2);
    expect(document.querySelector('.plot__y--right')).not.toBeNull();

    await user.click(screen.getByRole('button', { name: 'Energy' }));
    expect(screen.getByRole('button', { name: 'Heart rate' })).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByRole('button', { name: 'Energy' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('img', { name: /with energy over the night/ })).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Energy' }));
    expect(document.querySelectorAll('.plot__area svg path')).toHaveLength(0);
    expect(document.querySelector('.plot__y--right')).toBeNull();
  });

  it('shows no chips without any series, and none when stages are missing', () => {
    const { rerender } = render(<StagesWithOverlay night={night()} overlays={[]} />);
    expect(screen.queryByRole('group', { name: 'Draw over the stages' })).not.toBeInTheDocument();
    rerender(<StagesWithOverlay night={night({ stages_available: false, stages: [] })} overlays={OVERLAYS} />);
    expect(screen.queryByRole('group', { name: 'Draw over the stages' })).not.toBeInTheDocument();
    expect(screen.getByText('Shows after your watch records sleep stages for a night.')).toBeInTheDocument();
  });
});
