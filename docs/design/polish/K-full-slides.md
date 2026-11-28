# K. In focus slides that use their whole card

## Why

On the phone, Today starts with a swipeable row of four slides (Energy, Readiness, Sleep,
HRV). All slides are as tall as the tallest one (Energy: ring, flows, a day chart, a stress
strip and a legend). The owner: "It's just a square card in a bigger title. Before we had cards
that used the whole space to show more info. Just like the energy card." Readiness, Sleep and
HRV show a small block at the top and then empty space. Each slide must fill its card with real,
useful content, laid out like the Energy slide: a top block (ring or gauge + words), then one
wide chart, then supporting facts. Never "unavailable"; leave a missing part out, or one short
line when a whole part is missing.

All slides live in `web/src/pages/today/FocusCarousel.tsx` (visuals in `tileVisuals.tsx`,
styles in `today.css`). Keep the slide heights equal (they already stretch to the tallest);
aim for each slide's content to be about as tall as Energy's so none looks empty.

## Sleep slide

1. Top block (keep): score ring coloured by rating, "Sleep last night", rating word, time
   asleep, bed to wake times. Then the headline ("Long night").
2. Wide chart: the night's stage chart (`SleepVisual`, full card width, taller than the tile
   version, about 110 px) with the stage legend and each stage's duration under it. When the
   night has no stage data: leave the chart out and write one muted line "No sleep stages for
   this night." (no "unavailable").
3. Overnight facts, a row of three small stats: lowest/resting heart rate (`heart_rate`),
   overnight HRV (`hrv.last_night_ms`), breathing asleep (`respiration.sleep_avg_brpm`) — each
   only if present; Energy charged overnight if the payload has it.
4. "Last 7 nights": small bars of the sleep score per night (the nights query the desktop
   Sleep card uses — find it in `homeCards.tsx`/`TodayPage.tsx`), last night highlighted,
   day letters under the bars, and the 7-night average to the right.
5. Link "Open last night" at the bottom.

## Readiness slide

1. Top block: the readiness gauge (`ReadinessVisual`), level word, headline, "Nimrag
   estimate".
2. The six factors (`data.factors`, heaviest first): one row each with the factor name, its
   value in words/figures, and a short bar coloured by its rating (reuse what
   `web/src/pages/health/ReadinessView.tsx` renders if it fits; otherwise a compact version).
   A factor without data: its row says "No data" in muted text.
3. The sentence (`data.sentence`) and the link "What it is made from".
4. No estimate: the reason sentence plus the same factor list (whatever is present), so the
   owner sees what is missing.

## HRV slide

1. Top block: the big figure (last night, else the 7-night average), its label, "No reading
   last night" when missing, and the "within / below your range" words.
2. The range bar with the marker and the 28 nightly dots (`HrvVisual`, full width).
3. Wide chart: the overnight HRV of the last 4 weeks as a line with the usual range shaded
   (it had this before as a `SeriesChart` with `band`; bring it back under the dots), height
   about 90 px.
4. One line: "Your usual range is A to B ms." and a link "See HRV status" (/hrv).

## Energy slide

Unchanged.

## Checks

- App-mode screenshots (APP=1, 360 px) of each slide: click each dot and screenshot `.focus`
  (see the slides script below). Every slide's content should reach close to the card bottom;
  no large empty area.
- Also the no-data cases (no night, no estimate, no HRV) — still one calm card, not broken.
- Tests updated/added; typecheck, lint, vitest, build green.
