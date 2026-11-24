# Polish C: Today's cards with small charts, and the stress page (2026-11-24)

**1. "At a glance" cards get a small visual each** (phone `PhoneToday`/cards and desktop
`DesktopOverview`), under the value, about 40–56 px high, never with axes or labels:
- Heart rate: today's HR line (from the day's samples), resting value as a faint line.
- Stress: a ring (0–100, filled to the day's average, coloured by level) next to the number, or a
  day bar chart of stress coloured by level (rest / low / medium / high) — pick the ring for the
  card, the bars belong to the stress page.
- Training status: a strip of the last 28 days, one block per day coloured by status.
- Intensity minutes: a ring filled to this week's minutes against the weekly goal.
- Training load: a bar showing the 7-day load inside its target range (exists on the training page:
  reuse).
- Steps: the existing goal bar stays; add the last 7 days as 7 tiny bars.
- Recovery time: a ring that empties as the recovery time runs down.
- VO2 max: a sparkline of the last 3 months.
- Sleep (if a card exists for it): a ring with the score.
All from APIs the pages already use; add fields to the today API only where something is missing.
A card without data keeps its current empty text and shows no visual.

**2. Stress page (`/metrics/stress`):**
- Day view: the stress bars are coloured by level per bar (rest blue, low light orange, medium
  orange, high dark orange; activity/unmeasurable grey), with the same level thresholds the
  legend already uses (0–25 rest, 26–50 low, 51–75 medium, 76–100 high).
- A stacked horizontal bar "Time in stress levels" (rest/low/medium/high/activity) with minutes
  per level and percentages, above the existing tiles.
- One line of text under the chart that describes the day in plain words from the numbers (e.g.
  "Mostly resting; 40 minutes of medium stress, most of it between 14:00 and 16:00"): rule-based,
  own wording, no advice.
- 7d/4w/1y views: bars per day stacked by level minutes, the average as a line.

Tests: vitest for each card visual's presence with data and absence without, the level colouring
boundaries, the stacked bar sums, and the day sentence rules. Keep axe clean (the visuals are
decorative: `aria-hidden`, the numbers stay readable text).
