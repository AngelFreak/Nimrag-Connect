# Polish D: the sleep night page (2026-11-24)

Goal: one night's sleep explained, not just measured. Web pages `/sleep` (1d view; phone and
desktop layouts in `web/src/pages/health/SleepView.tsx` and the desktop variant). Own wording
throughout; this is Nimrag's own model built from public sleep guidelines.

## 1. Score header
Large score ring 0–100 with the level word and colour: Excellent 90–100, Good 80–89, Fair 60–79,
Poor below 60 (colour roles: Poor danger, Fair warning, Good success, Excellent a stronger success
tone; use existing tokens, add tokens only if needed, both themes). Next to it: time asleep
(light + deep + REM; awake never counts), bed and wake time.

## 2. Headline and one sentence
A short headline (2–6 words) and one sentence under the ring, chosen by our own rules from the
numbers, in this order of priority:
1. Time asleep vs sleep need (need = 8 h default; a per-user setting "Sleep need" in Profile →
   Goals, 6–10 h in 15 min steps, stored like the other goals): more than 1 h short → a "short
   night" headline; more than 1 h over → a "long night" headline.
2. Otherwise the weakest factor (section 3, lowest rating; ties: awake/restless, then deep, then
   REM, then stress, then light) names the night: e.g. restless, light/shallow, little deep sleep,
   little REM, tense (high stress).
3. All factors Good or better → a positive headline ("Solid, restful night" style).
The sentence says in plain words what stood out and one practical, non-medical hint linked to the
weakest factor (wind down, screens, caffeine late, late hard training, room temperature, regular
bedtime). Write about 15 headline+sentence pairs; keep them friendly and factual; never claim
medical effects. Put the rule table in one pure module with tests.

## 3. Score factors
A list of six rows, each: name, the night's value, a rating word with its colour dot (Excellent /
Good / Fair / Poor), and, when tapped/expanded, the target band in one line. Ratings by our model:
- Duration: asleep vs need: ≥ need Excellent; ≥ need−30 min Good; ≥ need−90 min Fair; else Poor
  (more than need+2 h: Fair).
- Stress: average sleep stress: ≤15 Excellent, ≤20 Good, ≤30 Fair, else Poor. Hidden when no value.
- Deep: share of time asleep, target 16–33 %: inside Excellent; within 4 points outside Good;
  within 8 Fair; else Poor.
- Light: target 30–64 %, same steps.
- REM: target 21–31 %, same steps.
- Awake and restless: awake minutes and the number of awakenings (and restless moments when the
  data has them): awake < 20 min and ≤1 awakening over 5 min → Excellent; < 30 min Good; < 45 min
  Fair; else Poor.
When the watch gave its own sub-scores for a factor (sleep_sessions: duration_score, deep_score,
light_score, rem_score, quality_score, recovery_score), show the watch's sub-score as the rating
instead (same 90/80/60 bands) and say "from your watch" in the expanded line; our model is the
fallback. Keep the existing stage chart and stage table.

## 4. Overnight strip
Under the stage chart, a grid of tiles (2 columns phone, 4 desktop), each shown only when the data
exists for that night: lowest / resting HR during sleep, average HR, energy (Body Battery) change
over the night (signed, "+50"), average and lowest SpO2, average and lowest breathing rate,
overnight HRV average, the 7-day HRV status word, awakenings, restless moments (from Garmin
Connect imports when present), average sleep stress.

## 5. Overlay on the stage chart
Chips under the stage chart pick one extra line drawn on the same time axis over the night:
Heart rate, Energy, Stress, Breathing, SpO2, HRV (only chips with data). Off by default; one at a
time; its own right-hand axis.

## Data
Server: extend the sleep night API with whatever is missing for the above (stage minutes exist; add
night-window series for the overlays and the strip values if not there; the sleep-need setting).
Only the signed-in user's data.

## Tests
Pure-module tests for every rating boundary and the headline rules (each branch); API tests for
new fields; web tests for the factor list, hidden tiles without data, overlay chips. Visual check at
412 and 1440 px in both themes.
