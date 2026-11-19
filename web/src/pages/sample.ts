// STATIC PLACEHOLDER DATA. The page stubs and /__gallery use the sample values drawn on the
// design canvas (docs/design/artifact/project). Nothing here comes from the API; the page
// stages (3c to 3m) replace these with generated API types and queries.

export const range = (n: number): number[] => Array.from({ length: n }, (_, i) => i);

/** Heart rate over 24 hours (bpm): low overnight, peak during the evening workout. */
export const HR_24H = [62, 58, 52, 49, 48, 50, 60, 72, 68, 76, 72, 82, 134, 148, 118, 86, 74, 68];

/** Overnight HRV (ms) for 30 nights: mostly 55 to 70, a dip nine days ago. */
export const HRV_30 = [60, 64, 57, 62, 66, 59, 55, 63, 65, 58, 54, 61, 64, 60, 56, 62, 63, 57, 48, 44, 52, 59, 63, 65, 58, 61, 66, 62, 59, 62];

/** Resting heart rate over 4 weeks (bpm). */
export const RHR_28 = [52, 51, 53, 52, 50, 52, 54, 52, 51, 53, 54, 52, 50, 52, 53, 52, 51, 55, 58, 60, 57, 54, 51, 51, 52, 51, 50, 51];

export const SLEEP_SCORE_28 = [82, 85, 78, 81, 86, 79, 75, 83, 85, 80, 74, 82, 87, 80, 77, 83, 84, 78, 70, 64, 72, 79, 83, 86, 81, 84, 88, 81];

export const BREATHING_28 = [13.4, 13.6, 13.3, 13.5, 13.4, 13.6, 13.2, 13.5, 13.4, 13.3, 13.6, 13.4];
export const SPO2_28 = [95, 96, 95, 96, 94, 95, 96, 95, 94, 95, 96, 95];

/** Sleep duration per night in minutes (7 nights, Thu to Wed). */
export const SLEEP_7 = [385, 412, 365, 399, 450, 372, 432];
export const SLEEP_14 = [405, 438, 372, 421, 445, 330, 395, 415, 440, 380, 421, 460, 405, 432];

export const STEPS_WEEK = [9120, 7480, 8360, 6240, null, null, null] as const;

export const LOAD_28 = [310, 320, 300, 330, 340, 320, 295, 310, 335, 345, 322, 305, 330, 312];
export const VO2_90 = [45, 45, 45, 45.5, 45, 46, 46, 46, 46];

export const ENERGY_DAY = [20, 48, 74, 94, 90, 84, 76, 62, 50, 44, 38, 40, 42];
export const WEIGHT_TREND = [82.4, 82.2, 81.9, 81.8, 81.5, 81.3, 81.2, 81.0, 80.8];
