# Polish A: the activity page (2026-11-24)

Goal: an activity page a runner opens after every run and finds everything they want at a glance.
Web only (phone layout = the app; desktop layout `ActivityDetailDesktop.tsx`). Both layouts get every
item below.

1. **Coloured route.** The route line is coloured by a chosen value along the way: Pace (or Speed
   for cycling), Heart rate, Elevation. A small segmented control above the map picks it (default
   Pace for runs/walks, Speed for rides, plain line for sports without GPS speed). Colour scale:
   a continuous ramp from a cool colour (slow / low) to a warm colour (fast / high), clipped to the
   5th–95th percentile of the activity so one GPS spike does not flatten the scale. A legend under
   the map shows the ramp with its two end values ("6:30 /km" … "4:50 /km"). Works with the
   "Route only" map setting (no tiles) too. Start and finish markers stay.
2. **Training effect in words.** Aerobic and anaerobic training effect (0.0–5.0) shown as a value,
   a 5-step bar and a word: 0–0.9 No effect, 1.0–1.9 Minor, 2.0–2.9 Maintaining, 3.0–3.9 Improving,
   4.0–4.9 Highly improving, 5.0 Overreaching. Plus the training load of the activity next to it.
   One card "Training effect" replaces the three separate tiles.
3. **Summary hierarchy.** The top of the page: sport icon, name, date/time; then a row of the 4–5
   key values for the sport in a larger, bolder figure (run: distance, time, avg pace, avg HR,
   ascent; ride: distance, time, avg speed, avg HR, ascent; strength: time, sets, reps, avg HR;
   other: time, avg HR, energy). Secondary values go to a "Stats" section grouped as Timing,
   Pace/Speed, Heart rate, Elevation, Running dynamics (cadence, stride…, only what exists),
   Energy. No empty values shown.
4. **Charts:** keep; add a shared hover/scrub cursor across the HR, pace and elevation charts on
   desktop (one vertical line and the values of all three at that point). Phone: tapping a chart
   shows the value at that point.
5. **Laps:** keep the table; on desktop add the lap's average HR colour dot by zone.

Data: everything above comes from what the activity API already returns (records with position,
speed, HR, altitude; session values; aerobic_te/anaerobic_te; load). If a value is missing from the
API, add it on the server in the smallest way.

Tests: vitest for the colour scale (percentile clipping, legend ends, pace inverted so fast = warm),
the TE words at each boundary, the summary choice per sport, and that empty values are hidden.
New/changed routes must be in `web/e2e/routes.ts`.
