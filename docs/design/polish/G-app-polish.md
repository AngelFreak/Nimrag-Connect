# Polish G: app look and feel (2026-11-24)

The Android app shows the web pages in dark theme at about 360 dp wide (`window.NimragApp.native`).
This pass makes them calmer and easier to read at a glance. Applies to the web too where the same
components are used (check light theme and desktop don't regress).

1. **Status colour everywhere a status word appears**: a small filled dot (or the word in the
   level colour on dark, whichever reads better) for training status, HRV status, sleep level,
   readiness level, stress level, energy level, on Today cards, the Health hub, page headers and
   lists. One shared component (`StatusWord` or similar) taking a kind and a key; the colour map
   lives in one place.
2. **Energy "In focus" card**: one chart, not two stacked: the energy line/area on top, and the
   day's stress as a thin band of level-coloured segments directly under it sharing the time axis
   (no second y-axis, no duplicate "100"). Legend on one line, short (Rest Low Medium High as dots),
   or dropped in favour of the colours explained on the stress page. The "woke" marker stays.
3. **Sub-lines on cards**: one short muted line under each value, never wrapping to three lines at
   360 px; alert colours (red) only when something is actually out of range. E.g. heart rate:
   "Resting · 45–74 today".
4. **Health hub charts**: the sentence under each chart repeats the header ("averaged 50 bpm,
   between 48 and 53"): show it as one compact muted line "48–53 bpm" in the header area and keep
   the full sentence for screen readers only (visually hidden). Same on other range cards.
5. **Typography rhythm**: values one consistent size per card type; labels sentence case, muted;
   section titles ("In focus", "At a glance", "Explore") same style everywhere; 8 px spacing grid;
   cards have equal heights per row on Today.
6. **Touch**: every tappable card shows a pressed state (subtle background change) and has at
   least a 44 px target; non-tappable cards don't look tappable (no chevron).
7. **Loading**: skeleton blocks shaped like the final cards instead of spinners or empty space,
   no layout shift when data arrives.
8. **Empty states**: one short line saying what will appear and when ("Shows after your watch
   syncs a night of sleep"), never "No data".

Check at 360 px with `window.NimragApp = {native: true}` injected (dark), at 412 px light, and at
1440 px light. axe clean. No change to numbers or features.

## Found while reviewing the first builds
9. Activity page, phone: the summary card repeats the page title and date right under the page
   header: drop the duplicate (keep the sport icon with the key figures). The hero figures need a
   clear gap between columns at 360 px (a 2- or 3-column grid with equal columns, not inline runs).
   The route colouring control should fit at 360 px without crowding ("HR" instead of "Heart rate"
   in the control if needed, full word for screen readers).
10. Stress colours: Medium and High are too close on dark; make them clearly distinct (High toward
    a deep red-orange or magenta-ish hue that still reads as "more"), check both themes and the
    legend.
11. Training status 28-day strip on the Today card: thinner gaps and runs of the same status drawn as
    one block, so it reads as periods rather than noise.
12. Sleep factors rated by the watch: the expanded line must explain the watch's rating ("Your
    watch scored this 72 of 100; Good is 80 or more"), followed by the target band for context;
    don't show Nimrag's own rule text ("within 4 points…") on a watch-rated row. The headline must
    not contradict the factor list: when Duration is watch-rated Good or better, don't open with
    "Short night" — fall through to the next rule (and say "a bit shorter than your need" in the
    sentence instead).
