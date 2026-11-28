# J. Today on the phone: a grid of visual tiles

## Why

The owner compared Today in the app with the official phone app and prefers its look:
every home card is a tile of equal width in a two-column grid, and each tile carries a
small, readable picture of its figure (a gauge, a ring split by level, a stage chart,
a range bar with a marker, a coloured badge) instead of a number with a thin sparkline.

They also could not find readiness: today the phone shows Energy, Readiness, Sleep and
HRV only as swipeable slides at the top, so a slide that is not the first is easy to miss.

## What to build (phone layout only; `web/src/pages/today/PhoneToday.tsx` and below)

1. **Energy stays the one wide card at the top** (the current Energy slide content, as a
   plain card, not a carousel). Remove the carousel and its dots from the phone page.
   Keep `FocusCarousel.tsx` only if something else still uses it; otherwise delete it with
   its tests.
2. **Below it, "At a glance" is a two-column grid of tiles** for every visible home card
   except Energy, in the owner's order (`settings.home_cards`, hidden cards left out).
   That now includes Readiness, Sleep and HRV. Each tile:
   - header row: the card's glyph in its tone + the card name (as now);
   - a visual of fixed height (about 120 px) centred in the tile;
   - one main figure with a short label under it;
   - the whole tile is one link to the card's page (as now), min height equal across a row;
   - an empty state in the same tile size when there is no data (short sentence, as now).
3. **The visuals**, one per card (draw them as small SVG components in
   `web/src/pages/today/tileVisuals.tsx`; reuse `Ring`, `RangeBar`, `StageBar`,
   `MiniBars` where they fit):
   - **Heart rate**: a 270° gauge. The track is split into coloured bands for the heart-rate
     zones (use the existing `--zone-1..5` tokens; zone edges from the user's zones if the
     Today payload has them, otherwise from the day's min..max split evenly), a dot marker
     at the latest reading, the latest bpm in the middle. Main figure: resting bpm,
     label "Resting".
   - **Stress**: a ring whose arc is split by the share of today's time at rest / low /
     medium / high (the `--stress-*` tokens), average in the middle. Under it a thin
     timeline of the day (00 to 24) with one bar per reading coloured by level, if the
     Today payload has the day's readings; if not, add them to the Today API (5-minute
     buckets are enough). Main figure: the level word.
   - **Sleep**: score (large) and duration side by side, then a stage chart of the night
     (blocks for deep / light / REM / awake at four heights, the existing sleep stage
     colours), with bed and wake times under its ends. If Today lacks the night's stage
     runs, add them (start, end, stage) to the Today API.
   - **HRV status**: the status word with a small coloured shape before it (use
     `StatusWord`), the 7-night average in ms as the main figure, a horizontal range bar
     split into the low / unbalanced / balanced / unbalanced zones with a marker at the
     7-night average, and under it a row of 28 small dots (one per night, coloured by that
     night's status; missing nights as hollow dots), label "Last 4 weeks".
   - **Readiness**: a 270° gauge split into five coloured segments for the five levels
     (poor → prime, the existing readiness level colours), a dot marker at the score, the
     score in the middle, the level word and the short headline under it, footnote
     "Nimrag estimate". No estimate: an empty tile that says what is missing (the API's
     `unavailable` sentence), never hidden.
   - **Training status**: a filled circle in the status colour with the status glyph in
     it, the status word large, "Since <date>" under it.
   - **Steps**: the number, a goal progress bar, and the last 7 days as small bars.
   - **Intensity minutes**: a ring against the weekly goal, minutes in the middle.
   - **Training load**: the 7-day load as the main figure and a range bar with the target
     range shaded and a marker at the load.
   - **Recovery time**: a ring of the time left, hours in the middle, "Ready around hh:mm".
   - **VO2 max**: the value and a sparkline of 3 months (as now), restyled to the tile.
   - **Health status, respiration, SpO2, weight**: keep their content, restyle into the tile.
4. **Readiness on the desktop**: the desktop layout keeps its structure, but its
   readiness card must also show the empty tile instead of vanishing when there is no
   estimate.

## Words and accessibility

Each visual gets an `aria-label` sentence with the exact figures (as the current
sparklines do); decorative parts `aria-hidden`. Colour is never the only carrier: the
level word is always written. Short plain texts in our own words.

## Checks

- App-mode screenshots at 360 px (APP=1) of Today, top to bottom, with demo data, and a
  day without data (empty tiles). Compare tile heights in a row: equal.
- Unit tests for the visuals (segments, marker angle at 0, middle and max, empty states).
- Existing Today tests updated; typecheck, lint, vitest, build all green.
