# Polish H: Today, Energy and Health status (2026-11-24)

## 1. Today
- **Readiness card** (In focus carousel and At a glance, phone and desktop): the estimate's ring,
  level word with colour, the headline, "Nimrag estimate" in small muted text; links to
  `/training/readiness`. Hidden when there is no estimate.
- **Sleep card** (At a glance): last night's score ring with level word, time asleep, the headline
  from the sleep page; links to `/sleep`.
- Both cards join the home card list so "Edit home" can order and hide them (existing mechanism;
  default order: Energy, Readiness, Sleep, then the rest).

## 2. Energy page (`/metrics/energy`, 1d)
- Header numbers: now, charged today (+), drained today (−), highest and lowest of the day.
- One combined day timeline: the energy line on top; under it, sharing the time axis, a band of
  stress level colours, sleep periods as a shaded block and activities as labelled blocks (sport
  icon, tap → activity).
- One plain sentence describing the day from the numbers (own rules): how much sleep recharged,
  what drained most (activities vs stress), where the day ends.
- 7d/4w: per day charged/drained bars (up/down from zero) and the daily high/low as a range band.

## 3. Health status (new page `/health/status`, add to e2e routes, linked from the Health hub
and as a Today glance card)
For each night, compare the overnight values with the person's typical range:
overnight HRV, resting HR during sleep, average breathing rate during sleep, average SpO2 during
sleep (when measured). Typical range per metric = from the previous 28 nights (needs ≥ 14 of
them): mean ± 1.5 × the standard deviation, at least ±1 unit (bpm, ms, brpm, %) wide.
- Page: per metric a row with last night's value, the range drawn as a band with the value as a
  dot, and "In range" / "Above" / "Below"; a summary line ("All in your usual range" or "2 values
  outside your usual range"); a 4-week chart per metric with the band.
- Calibrating state when fewer than 14 nights: shows how many nights are still needed.
- Clear note: this is a comparison with your own usual values, not a medical assessment.
Server: `GET /api/health-status?date=` and history `?from&to`; pure functions with tests (range,
calibration threshold, out-of-range rules), user isolation; ts-rs types.

Tests for each part; screenshots at 360 app mode and 1440 light; axe clean.
