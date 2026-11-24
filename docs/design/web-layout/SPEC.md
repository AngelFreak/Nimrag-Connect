# Web layout: Connect-style desktop (spec, 2026-11-24)

The owner asked for the web UI to follow Garmin Connect's web layout. This spec was measured
from Connect's web app at 1500 px CSS width (page geometry and computed styles only, no Garmin
code, assets or text). Reference screenshots are private: `private/connect-layout/*.jpg`.

Copy the **layout**: structure, grid, spacing, type scale, how pages are built. Do **not** copy
Garmin's wordmark, logo, icons, illustrations or copy text. Nimrag keeps its own NIMRAG wordmark,
its own icons (`web/src/components/icons.tsx`) and its own words.

Scope: the browser at >= 1024 px gets the new layout. Phones (< 1024 px) keep today's structure
(top bar + bottom nav) but take the new theme tokens. Inside the Android app (`isNativeApp()`)
nothing about navigation changes.

## Theme

Light is the web default. Dark stays (today's tokens) and is chosen with
`<html data-theme="dark">`; the native app forces dark. A profile setting "Appearance:
Light / Dark / Match system" stores the choice in `localStorage` (key `nimrag.theme`), applied
before first paint by an inline script in `index.html`.

Light tokens (Connect-measured):

| token | value | use |
|---|---|---|
| `--page` | `#efefef` | backdrop behind the content sheet |
| `--bg` / `--sheet` | `#ffffff` | content sheet, header |
| `--card` | `#ffffff` | cards |
| `--line` | `#e4e4e4` | card border (1px), dividers |
| `--line-strong` | `#c3c3c3` | stat-cell top rules, row separators |
| `--track` | `#dbdbdb` | segmented-control track, skeletons, empty icon discs `#e4e4e4` |
| `--text` | `#101010` | |
| `--text-2` | `#6c6c6c` | labels, captions |
| `--accent` / `--link` | `#1265c2` | links, primary buttons |
| `--sidebar` | `#1a1a1a` | sidebar background, sidebar text `#ffffff` |
| shadow | `0 4px 8px -4px rgb(0 0 0/.08), 0 2px 2px rgb(0 0 0/.1), 0 0 2px rgb(0 0 0/.1), 0 0 4px rgb(0 0 0/.12)` | cards |

Metric colours keep their roles but need light-background variants with >= 3:1 contrast
(e.g. heart `#e23b3b`, sleep `#7b5cff`, energy `#0aa58a`, steps `#c79a00`, stress `#f08a00`,
sleep stages deep `#1f6fd1`, light `#4aa3f0`, REM `#d42fbf`, awake `#f06ad8`).

Type: **Open Sans** (OFL, `@fontsource/open-sans`, weights 300/400/600). Body 14px/22px 400.

| role | size / weight |
|---|---|
| page title | 40px / 300 |
| section title ("In Focus", "Sleep Score Factors") | 22px / 300, padding 5px 0 11px |
| big stat value (cards) | 32px / 300 |
| headline stat value (activity) | 24px / 300 |
| stat value (grids) | 18px / 400 |
| stat label | 12px / 400, `--text-2` |
| card title | 12px / 400 with a 20px icon before it |
| list stat label (activity rows) | 9px / 600 uppercase, letter-spacing .04em |
| sidebar item | 16px / 300 (top level), 14px / 300 (sub items) |
| button | 12px / 600, radius 4px, padding 4px 16px, height 30px |

Radii: card 8px, button 4px, segmented track 4px, round icon buttons 50%.

## Shell (>= 1024 px)

```
+------------------+---------------------------------------------------------------+
| NIMRAG (64 tall) | (<) collapse            header 60px, white, 1px bottom line   (sync)(watch)(avatar) |
|------------------+---------------------------------------------------------------+
| sidebar 264px    | backdrop --page, padding 0 30px                               |
| #1a1a1a          |   sheet: white, padding 56px, max-width 1152px                |
| groups separated |     page content                                              |
| by 1px #333      |                                                               |
+------------------+---------------------------------------------------------------+
```

- Sidebar is fixed, full height, scrolls on its own. Top: wordmark area 64px tall, 20px left pad.
- Item: 40px tall, padding 8px 15px 8px 20px, 24px icon slot then 10px gap, label. Group heads have a
  chevron at the right (rotates when open). Sub items: 31px tall, 14px, indented to the label (52px).
- Active item: 4px solid white left border and background `rgb(255 255 255/.08)`.
  Active sub item: background `rgb(255 255 255/.08)`, no border. Hover: `rgb(255 255 255/.05)`.
- The group that contains the current page is open on load; groups toggle on click (state kept in
  `localStorage` per group).
- Collapse button (32px circle, 1px border, chevron) in the header's left; collapsed sidebar = 64px
  icon rail with tooltips. Remember the choice in `localStorage`.
- Header right: Sync button, Watch button (existing `SyncAndWatch`), profile avatar (existing
  `ProfileLink`), 40px apart, icons 20px.

Nimrag's sidebar (only pages that exist):

- Home `/` · Calendar `/calendar`
- ── divider
- **Activities** ▸ All Activities `/activities` · Strength `/strength` · Steps `/metrics/steps` ·
  Intensity Minutes `/metrics/intensity_minutes` · Movement `/movement`
- **Health Stats** ▸ Sleep `/sleep` · Heart Rate `/metrics/heart_rate` · Stress `/metrics/stress` ·
  Energy (Body Battery role) `/metrics/energy` · Respiration `/metrics/respiration` ·
  Pulse Ox `/metrics/spo2` · Weight & Body `/body` · Day `/day`
- **Performance Stats** ▸ Training Status `/training/status` · HRV Status `/hrv` ·
  Training `/training` · Training Load `/metrics/training_load` · VO2 Max `/metrics/vo2_max`
- **Training & Planning** ▸ Workouts `/workouts`
- ── divider
- Watch `/devices` · Goals `/profile/goals` · Import `/profile/import-garmin`

`/health` (the phone hub) stays reachable but is not in the sidebar.

## Page patterns

### A. Stats page (Sleep, HRV, Training status, every `/metrics/*`, Body, Movement, Day, Training)

1. Title row: page title (40/300) left; help `?` and kebab icons right (only if the page has them).
2. Toolbar row, 24px below: left = prev / next round buttons (36px, `--track` bg, disabled = 50%)
   and a date chip (36px tall, `--track` bg, radius 4px, calendar icon + bold date or range
   "Nov 18 - Nov 24"); right = segmented range control (track `#dbdbdb` radius 4px, 4px inset,
   selected segment white with shadow, each segment ~120px, 14px text, selected 600).
   Labels: "1 Day", "7 Days", "4 Weeks", "12 Weeks", "6 Months", "1 Year".
3. 1px `--line` divider, 40px below the toolbar, then 48px space.
4. Content. One-day view: two columns, left 320px (score block, headline + paragraph, notes), right
   flexible with 56px gap (factor rows, charts). Multi-day view: one column of sections, each a
   section title, a chart, and a **stat strip**.
5. **Stat strip**: a row of cells, each 145-165px wide with 16px gap, a 1px `--line-strong` top rule,
   value 24/300 on top, label 12px `--text-2` below. Wraps to a 4-column grid.
6. **Factor row**: 1px `--line` box, radius 4px, padding 10px 12px, 64px tall, name 16/400 + sub 12px
   grey on the left, rating 16/300 right-aligned. 32px between rows.
7. Charts: no card around them; grid lines `--line`, axis labels 12px grey, legend under the chart
   with 12px dots, "pill" toggles (1px border, radius 999px, 12px text, 4px 10px) for extra series.

### B. Home (`/`)

Grid: two columns, `672px` and `312px`, gap 48px rows / 56px columns. "Synced 11m ago" style status
12px grey right-aligned above.

Left column, top to bottom:
1. **In Focus** (section title). 2-up cards, 332 x 304, gap 8px. Each card: 12/20 title with icon
   (and a round `(+)` action at right when it has one), big value 32/300, two side values (value
   14px + label 12px grey) at the right of the big value, a chart filling the middle, a 1px divider
   and a footer row (e.g. "Last 4w" dot strip) at the bottom. Nimrag's In Focus: Energy, All
   activities this week (bar per day), Running this week, Steps, Sleep score, Training status.
2. **At a Glance** with a "See All" link right. 3-up small cards 185 x 257, gap 8px: title with icon,
   value(s), mini chart or ring; optional footer with divider. Nimrag: Heart rate, Stress (ring),
   Sleep score, HRV status, Training readiness/recovery, Training status, Intensity minutes, Training
   load, VO2 max, Steps.
3. **Yesterday**: table rows 52px, 1px `--line` between, icon 20px + blue link name (14px) in col 1
   (200px), value in col 2, optional right progress bar (steps vs goal, green `#0a7d3e` bar 4px).
4. **Last 7 Days**: same row style, 2 columns (name / value).
5. "Edit Home" grey button (`--track` bg, 12/600).

Right column: **Recent activities** (cards like the activity list rows but compact), **Sync and
watch** card (today's "Sync and storage" content), **Workouts** card (next/recent workouts, link to
`/workouts`).

### C. Activities list (`/activities`, `/strength`)

1. Title row: "Activities" 40/300 left; right text links "+ Workout", "Import", "Export CSV" (14px,
   `--text`, 24px apart) — only for actions Nimrag has.
2. Toolbar: search box (260 x 36, 1px border, search icon right) — filters by name/type client-side
   over loaded rows; then a segmented icon filter (blue `#1265c2` selected, white others, 1px border,
   each 48 x 36): All, Run, Cycle, Walk, Hike, Swim, Strength, Training, Other.
3. Rows as cards (radius 8px, card shadow, 64px tall, 8px apart). Columns: round sport icon 40px
   (white glyph on the sport colour: run orange `#f27a1a`, cycle green `#16a34a`, walk/hike teal,
   strength purple, other red) · date stacked ("Nov 23" 14/400, year 9px) 80px · name 14px link +
   sport 9px uppercase grey under it (flex) · 5 stat columns 100px each, value 14px + label 9/600
   uppercase grey (Distance, Time, Avg pace or speed, Total ascent or Sets, Avg HR or Calories).
4. "Load more" stays at the end.

### D. Activity detail (`/activities/:id`)

Two columns: main 745px, side 350px, gap 24px.

Main:
1. Breadcrumb line: `RUNNING · ON NOVEMBER 8, 2026 @ 17:22` 12px uppercase letter-spaced grey;
   prev/next activity arrows at the right of the main column.
2. Title row: 56px round grey icon disc (`#e4e4e4`) with the sport glyph, title 26/300, edit pencil;
   a meta line under it (12px grey) with device / type.
3. Headline stats: 5 equal cells, value 24/300, label 12px grey. 24px below the title.
4. Map (existing `MapView`), 390px tall, radius 0, a "Slower ▭ Faster" legend under it.
5. Chart stack: each chart = header (12px coloured dot + 14px title, expand icon right), 120px chart
   with an "Avg: 4:36 /km" dashed line label at right, x axis in time. Time / Distance segmented
   control above the stack, right-aligned.
6. Tabs: Stats · Laps · Time in Zones (and Sets for strength), 14px, active = 2px `--text` underline.
7. Stats tab: two columns of groups. Group = 14/400 title with a 1px rule under it, then items
   (value 18/400, label 12px grey, 12px between items).

Side: Notes card (textarea placeholder), Device card (watch name, software), Weather, Training
effect / load summary.

## What stays

Data, queries, routes, accessibility (skip link, focus on route change, aria-current), phone layout
structure, native-app behaviour, tests' intent. Update tests for new markup rather than deleting them.
