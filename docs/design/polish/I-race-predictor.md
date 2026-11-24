# Polish I: race predictor (Nimrag's estimate) (2026-11-24)

Predicted finish times for 5 km, 10 km, half marathon (21.0975 km) and marathon (42.195 km),
clearly labelled "Nimrag estimate" (our own model from public formulas, not Garmin's).

## Model (server, pure functions with tests)
1. **From recent best efforts** (the personal-records best-effort cache, `activity_efforts`): take
   the person's best efforts from runs in the last 42 days at 1 km, 1 mile, 5 km, 10 km, half,
   marathon. For each target distance D, predict from every effort (d, t) with Riegel:
   `T = t × (D/d)^1.06`, using only efforts with d ≥ 1 km and d ≤ 2.5 × D… prefer the effort whose
   distance is closest to D (in log distance) among the three fastest predictions; take the median
   of those three predictions.
2. **From VO2 max** (the watch's running VO2 max if present, else Nimrag's derived value): Daniels &
   Gilbert: velocity v (m/min) for a race of duration t (min) satisfies
   `VO2max = (-4.60 + 0.182258 v + 0.000104 v²) / (0.8 + 0.1894393 e^(-0.012778 t) + 0.2989558 e^(-0.1932605 t))`;
   solve for t at each distance by bisection.
3. **Blend:** when both exist, 70 % best-effort prediction + 30 % VO2 max prediction; one alone when
   only one exists; none when there are no runs with GPS distance in the last 42 days and no VO2 max.
Also give the pace per km for each prediction. History: one value per week (computed as of each
Sunday) for the last 26 weeks.

API: `GET /api/race-predictions` → `{as_of, basis: "efforts"|"vo2max"|"both", predictions: [{distance_m,
seconds, pace_s_per_km}], history: [{date, predictions: [...]}]}`, signed-in user only, ts-rs types.

## Web
Page `/training/race-predictor` (add to `web/e2e/routes.ts`), linked from the Training page and the
Health hub list:
- Four rows/cards: distance name, predicted time (h:mm:ss or m:ss, whole seconds), pace /km, and the
  change vs 4 weeks ago (e.g. "−0:42 since 4 weeks", green when faster).
- A history chart: tabs 4w / 6m with chips to pick the distance (default 5 km); y axis as time,
  inverted so faster is up.
- A short note on what the estimate is based on ("Based on your best efforts in the last 6 weeks
  and your VO2 max") and the label "Nimrag estimate — not Garmin's Race Predictor".
- Empty state: one line saying a run with GPS in the last 6 weeks is needed.
Phone 360 px and desktop; both themes; axe clean.

Tests: Riegel and Daniels functions against known values (e.g. VO2max 50 → 5 km ≈ 19:57, 10 km ≈
41:21, half ≈ 1:31:35, marathon ≈ 3:10:49, within ±15 s), the blend and basis selection, the 42-day
window, history weeks, user isolation; web formatting and empty state.
