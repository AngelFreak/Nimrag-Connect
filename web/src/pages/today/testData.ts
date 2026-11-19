// Fixtures for the Today page tests: shaped like the API's JSON (types come from the generated files).
import type { SettingsResponse, Today } from '../../api/generated';
import { DEFAULT_HOME_CARDS } from './cards';

const T = (time: string, value: number | null) => ({ time, value });

export function makeToday(overrides: Partial<Today> = {}): Today {
  const base: Today = {
    sample: true,
    date: '2026-11-19',
    generated_at: '2026-11-19T10:00:00+00:00',
    timezone: 'Etc/UTC',
    last_sync_at: '2026-11-19T07:42:00+00:00',
    last_data_at: '2026-11-19T07:40:00+00:00',
    energy: {
      now: 68,
      now_at: '2026-11-19T10:00:00+00:00',
      charged: 54,
      drained: 31,
      high: 100,
      low: 40,
      wake_time: '2026-11-19T06:36:00+00:00',
      series: [T('2026-11-19T00:00:00+00:00', 40), T('2026-11-19T06:36:00+00:00', 95), T('2026-11-19T10:00:00+00:00', 68)],
    },
    sleep: {
      date: '2026-11-18',
      start: '2026-11-18T23:30:00+00:00',
      end: '2026-11-19T06:36:00+00:00',
      duration_s: 25920,
      time_in_bed_s: 26400,
      score: 81,
      score_label: 'good',
      scores: { overall: 81, quality: 80, duration: 85, recovery: 75, deep: 90, light: 88, rem: 70 },
      awakenings: 1,
      avg_stress: 18,
      stages_available: true,
      deep_s: 6000,
      light_s: 12000,
      rem_s: 7000,
      awake_s: 600,
      stages: [],
      hrv_ms: 62,
      lowest_hr_bpm: 46,
      breathing_brpm: 13.4,
      spo2_avg_percent: 95.8,
      spo2_low_percent: 93,
    },
    hrv: {
      last_night_ms: 62,
      weekly_avg_ms: 64,
      baseline_low_ms: 55,
      baseline_high_ms: 70,
      status_code: 4,
      status: 'balanced',
      vs_baseline: 'within',
      values_4w: [
        { date: '2026-11-17', value: 60 },
        { date: '2026-11-18', value: 63 },
        { date: '2026-11-19', value: 62 },
      ],
    },
    heart_rate: { resting_hr_bpm: 51, avg_7d_resting_hr_bpm: 52, day: { low_bpm: 48, high_bpm: 148, series: [] } },
    heart_rate_24h: { low_bpm: 48, high_bpm: 148, series: [] },
    stress: { average: 23, main_level: 'rest', rest_s: 3600, low_s: 1800, medium_s: 0, high_s: 0, activity_s: 600, series: [] },
    steps: { steps: 6240, goal: 8000, percent_of_goal: 78, distance_m: 4800 },
    intensity_minutes: { week_start: '2026-11-16', moderate_min: 30, vigorous_min: 20, total_min: 70, goal_min: 150, days: [] },
    training_status: { status_code: 4, label: null, since: '2026-11-01', since_is_lower_bound: false, fitness_trend_code: 1, load_trend_code: 1, as_of: '2026-11-19T06:30:00+00:00' },
    training_readiness: null,
    training_load: { seven_day_load: 306, target_min: 230, target_max: 403, vs_target: 'within', acute: 306, chronic: 288, ratio: 1.1, series_4w: [] },
    respiration: { latest_brpm: 15.2, latest_at: '2026-11-19T09:50:00+00:00', day_avg_brpm: 15, sleep_avg_brpm: 13.4 },
    spo2: { latest_percent: 97, latest_at: '2026-11-19T06:30:00+00:00', sleep_avg_percent: 95.8, sleep_low_percent: 93 },
    recovery_time: { started_at: '2026-11-18T18:00:00+00:00', total_s: 40800, remaining_s: 26852, percent_complete: 34, ready_at: '2026-11-20T05:43:00+00:00' },
    vo2max: { value: 46.2, maximal_met: 13.2, derived: true, measured_at: '2026-11-18T17:47:00+00:00', change_3m: 0.5, series_3m: [] },
    weight: { weight_kg: 77.6, measured_at: '2026-11-19T07:10:00+00:00', change_30d_kg: -0.5 },
    last_activity: null,
    training_this_week: { sessions: 3, duration_s: 9060 },
    yesterday: {
      date: '2026-11-18',
      energy_charged: 54,
      energy_drained: 31,
      steps: 9120,
      sleep_score: 81,
      sleep_duration_s: 25920,
      resting_hr_bpm: 51,
      hr_high_bpm: 148,
      stress_avg: 24,
      hrv_7d_avg_ms: 64,
      intensity_minutes: 42,
    },
    last_7_days: {
      from: '2026-11-12',
      to: '2026-11-18',
      activity_count: 4,
      activities_by_kind: [
        { kind: 'run', count: 2 },
        { kind: 'strength', count: 2 },
      ],
      avg_steps: 8441,
      avg_sleep_score: 74,
      avg_sleep_duration_s: 25500,
      avg_resting_hr_bpm: 52,
      avg_stress: 22,
    },
  };
  return { ...base, ...overrides };
}

/** Every metric the watch could miss, missing. */
export function makeEmptyToday(): Today {
  const t = makeToday();
  return makeToday({
    last_sync_at: null,
    last_data_at: null,
    energy: { now: null, now_at: null, charged: null, drained: null, high: null, low: null, wake_time: null, series: [] },
    sleep: null,
    hrv: { ...t.hrv, last_night_ms: null, weekly_avg_ms: null, baseline_low_ms: null, baseline_high_ms: null, status: null, status_code: null, vs_baseline: null, values_4w: [] },
    heart_rate: { resting_hr_bpm: null, avg_7d_resting_hr_bpm: null, day: { low_bpm: null, high_bpm: null, series: [] } },
    stress: { ...t.stress, average: null, main_level: null, series: [] },
    steps: { steps: null, goal: 8000, percent_of_goal: null, distance_m: null },
    intensity_minutes: { ...t.intensity_minutes, moderate_min: null, vigorous_min: null, total_min: null },
    training_status: { status_code: null, label: null, since: null, since_is_lower_bound: false, fitness_trend_code: null, load_trend_code: null, as_of: null },
    training_load: { ...t.training_load, seven_day_load: null, target_min: null, target_max: null, vs_target: null },
    respiration: { latest_brpm: null, latest_at: null, day_avg_brpm: null, sleep_avg_brpm: null },
    spo2: { latest_percent: null, latest_at: null, sleep_avg_percent: null, sleep_low_percent: null },
    recovery_time: { started_at: null, total_s: null, remaining_s: null, percent_complete: null, ready_at: null },
    vo2max: { value: null, maximal_met: null, derived: true, measured_at: null, change_3m: null, series_3m: [] },
    weight: { weight_kg: null, measured_at: null, change_30d_kg: null },
    yesterday: { ...t.yesterday, energy_charged: null, energy_drained: null, steps: null, sleep_score: null, sleep_duration_s: null, resting_hr_bpm: null, hr_high_bpm: null, stress_avg: null, hrv_7d_avg_ms: null, intensity_minutes: null },
    last_7_days: { ...t.last_7_days, activity_count: 0, activities_by_kind: [], avg_steps: null, avg_sleep_score: null, avg_sleep_duration_s: null, avg_resting_hr_bpm: null, avg_stress: null },
  });
}

export function makeSettings(overrides: Partial<SettingsResponse> = {}): SettingsResponse {
  return {
    timezone: 'Etc/UTC',
    step_goal: 8000,
    intensity_goal_min: 150,
    home_cards: { order: [...DEFAULT_HOME_CARDS.order], hidden: [...DEFAULT_HOME_CARDS.hidden] },
    sample: true,
    ...overrides,
  };
}
