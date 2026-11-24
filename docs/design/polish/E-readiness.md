# Polish E: readiness (Nimrag's estimate) and training status explained (2026-11-24)

The Instinct 2 writes training status, load, recovery time and VO2 max itself (Nimrag already
shows them) but no readiness score. Nimrag computes its own, clearly labelled as an estimate.

## 1. Readiness estimate
Score 0–100 each morning (after the night's sleep is in; recomputed when new data arrives), from
six factors, each rated on five rungs (very poor 0, poor 25, moderate 50, good 75, very good 100):
- Sleep score of last night (90+ very good, 80+ good, 60+ moderate, 40+ poor, else very poor).
- Sleep history: average score of the last 7 nights, same bands.
- Recovery time left now (watch value): 0 h very good, ≤12 h good, ≤24 h moderate, ≤48 h poor,
  more very poor.
- HRV status: 7-day overnight HRV average vs the person's baseline band (as on the HRV page):
  inside/balanced good (very good when also ≥ last night above the band's middle), unbalanced
  moderate, low poor, no baseline yet → factor missing.
- Acute load vs its target range (as on the training page): below range very good, in the lower
  half good, upper half moderate, above poor, far above (>1.5× top) very poor.
- Stress history: average all-day stress of the last 3 days: ≤25 very good, ≤35 good, ≤45
  moderate, ≤55 poor, else very poor.
Score = weighted mean of present factors (weights: sleep 25, recovery 20, HRV 20, load 15, sleep
history 10, stress 10, renormalised over the present ones); none shown with fewer than 3 days of
data or fewer than 4 factors present. Levels: 0–24 Poor, 25–49 Low, 50–74 Moderate, 75–94 High,
95–100 Prime (colour roles bad→good). One headline (2–5 words) + one sentence from our own rules:
level plus the weakest factor (e.g. low → "Take it easy today" naming the weakest factor; high →
ready for a hard session). Own wording, about 12 pairs, pure module with tests.

Server: compute in Rust from existing tables (a new derived module; cache per user per day, cleared
when that day's inputs change); `GET /api/readiness?date=` → score, level, factors [{key, value,
unit, rating, weight}], headline key; `GET /api/readiness/history?from&to` → per day score/level.
ts-rs types.

Web: a page `/training/readiness` (add to web/e2e/routes.ts): a ring with score and level, the
label "Nimrag estimate — not Garmin's Training Readiness", headline and sentence, the six factor
rows (value + rating word + colour), and a history chart (7d/4w/12w tabs, bars coloured by level).
Link it from the Health hub list and the Training page (a card at the top). Do NOT edit Today page
files (`web/src/pages/today/*`): another builder is changing them; a Today card comes later.

## 2. Training status explained
On the Training status page (`/training/status`, `TrainingStatusView.tsx`) and the Training page:
- One sentence under the status naming why, from our own rules on what Nimrag has: status (from the
  watch), acute load vs target range, the VO2 max trend over 4 weeks (up / flat / down), HRV
  status. Patterns: productive = load right and fitness rising; maintaining = load enough to hold;
  recovery = light load, fitness steady; detraining = much less training than usual; overreaching
  = load too high; unproductive = load fine but fitness falling (rest, sleep, stress); strained =
  HRV shows fatigue; peaking = load eased after hard training, fitness high; add an HRV caveat
  when HRV is unbalanced. About 15 sentences, own wording, pure module with tests.
- Factor rows: VO2 max (value + 4-week change), acute load (value + Low/Optimal/High/Very high vs
  range), HRV (7-day average + status word), load focus (the three intensity buckets with which
  one is short of its target, from the existing load-by-intensity data), each linking to its page.
- A one-paragraph "What this status means" (expandable) per status, own words.

Tests: rating boundaries, score weighting/renormalisation, missing-data rules, headline and
explanation rules, API isolation per user, page rendering with and without data.
