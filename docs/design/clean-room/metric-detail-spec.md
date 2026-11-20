# Metric detail screens: functional specification (clean room)

## Provenance

| Item | Value |
| --- | --- |
| Subject | Garmin Connect for Android, the owner's own copy |
| Package / version | 5.31.1 (versionCode 11765), split APK set |
| SHA-256 | `78f906106af4eb4622f44ed5a6fd161e971b0885d809898ff68192ae77083766` |
| Signer (v2 and v3, as declared) | Garmin International, certificate SHA-256 `84ff2bfa395c5a21e53ae29633e4a6e68b6b5c6707bb6084c25b70f866b71729` |
| Date | 2026-11-20 |
| Tools | REA 6.9.0; jadx-core from REA's JADX provider jar (jadx-headless-mcp 0.7.1), run per dex file because the whole APK does not fit in memory on this host; small Python readers for dex string/class tables, `resources.arsc` strings and dex bytecode (for classes JADX could not decompile) |
| Method | Static reading only. The app was not run, nothing was installed, adb was not used. |

The APKs, decompiled sources, the analyst's scratch notes and the full spec with its Evidence
lists (class and endpoint names for a reviewer) are in `private/re/gc-5.31.1/` (git-ignored) and
are not redistributed. This copy is the builder's: Evidence lists, internal names and analyst
notes removed.

## Clean-room rules

This document is written by the analyst for a builder who never sees the Garmin app or its
code. It describes behaviour in our own words. It contains no Garmin source code, layouts,
resource files, artwork or copied help text. On-screen labels appear only as short functional
labels of one to three words (for example "Resting" or "7d"). Colours are given as roles
(for example "goal reached" or "high stress"); the builder chooses the palette. Help and
"about" content is summarised as one-line topics. Class, method, resource and JSON field names
appear only in the Evidence lists and data hints, so that a reviewer can check a finding; they
are names, not code. Numeric norm tables that the app embeds (VO2 max categories) are not
reproduced; the builder takes such tables from an openly published source. Where the analyst
could not see something it says "Unknown" and why; "(inferred)" marks a conclusion drawn from
surrounding code rather than read directly.

Analysis note: R8 obfuscated most of the app. Original names come from Kotlin metadata (debug
class names, data-class `toString`). Some classes did not decompile and were read from the dex
bytecode, which shows calls, constants and string ids but not full control flow; statements
based on that are marked "(inferred)".

## Common pattern of the detail screens

Most screens below share one frame. Screen sections only describe how they differ.

- **Top bar**: metric title, overflow menu (help, and per screen: edit goal, add or delete
  entries).
- **Range tabs**: a segmented row. The available tabs are chosen per screen from this set:
  "1d", "7d", "4w", "12w", "6m", "1y" (plus "Current" on the performance screens). The default
  is 1d unless the opening card passes a tab. The card can also pass a start date.
- **Periods**: day-based tabs are rolling windows of N days that end on the anchor date (today
  on the first page): 7d = 7 days, 4w = 28 days, 12w = 84 days, 6m = 182 days, 1y = 364 days
  (52 weeks). They are not calendar weeks or months, except where a screen says so (Intensity
  Minutes 7d). Month-based ranges, where used, are aligned to the first day of a calendar month
  and the newest one contains the current month.
- **Moving**: a period header with previous and next chevrons and the period label between
  them (one date when the period is a single day, otherwise "start – end"). Previous moves back
  by exactly one window length. The content is also a horizontal pager, so swiping does the
  same. Paging into the future is not possible (inferred).
- **Drill-down**: tapping a day row on 7d/4w opens that day on 1d; tapping a week row on 1y
  opens the 7d tab on that week. The screen switches tab and jumps to the page that contains
  the date.
- **Layout per page**: chart card (title, chart, optional footer of summary tiles), then
  summary numbers, then a list of rows for the period, then help and extra cards.
- **Landscape**: only the chart is shown, full screen, with the date as its title.
- **Loading and errors**: each page has its own loading state and an error state with retry.
- **1-day data**: Heart Rate, Stress and Body Battery read the day from one combined
  "timeline for a date" response that
  carries all-day heart rate, stress and Body Battery samples, zones, activities and the daily
  summary. Timeline events (activities, sleep, auto-detected moves) are drawn as small icons at
  their minute on the 0–1440 minute x axis of the 1-day charts.
- **Multi-day data**: mostly a server endpoint for
  7d/4w and `.../weekly/{end}/{numWeeks}` for 1y (52 weeks), with metric-specific services
  for sleep, HRV, weight and the performance metrics. Endpoint names are hints of what the
  server already aggregates; Nimrag computes the same values from its own stored data.

## Screens covered

1. Heart Rate
2. Stress
3. Body Battery
4. Steps
5. Floors
6. Calories
7. Intensity Minutes
8. Sleep
9. HRV Status
10. Respiration
11. Pulse Ox
12. Training Readiness
13. Training Status
14. Training Load / Acute Load (incl. Load Focus)
15. VO2 Max
16. Recovery Time
17. Weight

## Heart Rate

### Navigation

- Top bar title: "Heart Rate", with a back arrow.
- Tabs: **1d, 7d, 4w, 1y**, shown in portrait only. The default is **1d**.
  - The launching card can pass a start date and a range tab.
- Overflow menu:
  - On the 1d tab it has up to three items:
    - Help.
    - "Abnormal HR settings": only when the user has at least one device that supports
      abnormal HR alerts. It opens the device setting for those alerts.
    - "Delete abnormal HR": only when the selected day has abnormal-HR alerts. It opens a
      bottom sheet listing that day's alerts. Picking one opens a confirmation dialog
     , then calls a server endpoint.
  - On the other tabs the menu has only Help.
- Drill-downs:
  - 7d / 4w: tapping a day row opens the **1d** tab on that day.
  - 1y: tapping a week row opens the **7d** tab on the page that contains that week's start
    date.
  - 1d: tapping an event row opens its detail screen through. The target depends on the
    row: activity detail, auto-detected move detail, ECG detail or health-monitoring detail.

### Charts

**1d tab: "Daily Timeline" card**

The card title is "Daily Timeline" in portrait and the date in landscape. The chart is
, a.

- X axis: minutes since the day's `startTimestampGMT`, from 0 to 1440. Hour labels are drawn
  by the shared day axis.
- Series: one HR line.
  - Points come from `allDayHeartRateDTO.heartRateValues` as [timestamp, bpm] pairs, sampled
    every 2 minutes.
  - The line breaks wherever a value is null or 0.
  - The line colour is a vertical gradient by HR zone. The zone boundaries come from the
    day's heartRateZonesDTO. Colour roles: zone 1 to zone 5 colours.
- Y range:
  - Max = round up to the next 10 of (max(top zone boundary, day max HR) + 5). The default is
    200 when there is no data.
  - Min = `lo8.F(minHR, max)`: round down to 10 of (0.3 x max - 15). If round down to 10 of
    (min - 15) is lower, use that instead. Never go below 20.
- Tooltip (press and hold):
  - The time range t to t+2 min.
  - The value with "bpm".
  - The label "Heart Rate".
  - The zone number, labelled "Zone". Zone 0 shows "<1". The tooltip uses the zone
    colour.
- Abnormal-HR markers: each point in `abnormalHRValuesArray` gets an alert icon on the line,
  labelled "Alert". Its tooltip adds "Low" if the value is 60 or less, or "High" if it is 100
  or more.
- Limit lines: dashed lines in the alert colour role, drawn only when the value is > 0:
  - at `abnormalHrThresholdValue` (high threshold);
  - at `lowAbnormalHrThresholdValue` (low threshold).
- Legend:
  - an "Abnormal Alert" icon, only when the day has alerts;
  - "High HR (N bpm)" and "Low HR (N bpm)" as dashed entries, only when those thresholds exist.
- Timeline event icons on the x axis.

**7d / 4w tabs: "Daily Readings" card**

Chart config with daily data `loa`. It is a.

- X axis: one point per day (step 1). Labels are shown for the first and last date only
 .
- Series: two lines with dots, each split wherever a day is missing:
  - **High**: the day's `wellnessMaxAvgHR`. Colour role: high-HR (red family).
  - **Resting**: the day's `restingHR`. Colour role: resting-HR (blue family).
  - Only values > 0 are plotted.
- Y range:
  - Max = round up to the next 10 of (max(max resting, max high) + 5). The default is 200.
  - Min uses the same rule as 1d. It is 0 when there is no data.
- Tooltip: the date, the value with "bpm", and the label "Resting" or "High".

**1y tab: "Weekly Averages" card**

Same config, with weekly data.

- X axis: one point per week (step 7, no dots). Labels are month names.
- Series: the same two lines.
- Tooltip: the week range, the value with "bpm", and the label "Avg Resting" or "Avg High".

### Summary numbers

- 1d: the card footer (portrait only, case 8) shows two stats:
  - "Resting" = the day's `restingHeartRate`;
  - "High" = the day's `maxHeartRate`.
  - Both are in bpm.
- 7d / 4w: the card footer shows two stats:
  - "Avg Resting" = the mean of the daily restingHR values that are > 0, rounded to an
    integer;
  - "Avg High" = the mean of the daily wellnessMaxAvgHR values that are > 0, rounded
   .
  - A value of 0 or less shows "--".
- 1y: the same two averages, computed over the weekly values (`gxi` case 19 ->).

### Lists

- **1d, under the chart (portrait only):**
  - Abnormal-alert block: shown only when the day has alerts. It has a heading for
    abnormal HR alerts, then one row per alert with the time of day and "N bpm".
  - Event rows, in this order (ordering inside each group inferred to be by time):
    - Activities of the day: name/type, with tap-through to the
      activity.
    - Auto-detected moves that do not overlap an activity, with their duration.
    - Wellness activities: ECG rows show the duration and "Avg HR". Other
      health-monitoring sessions show the average HR.
- **7d / 4w:** one row per day, newest first.
  - Title: the weekday.
  - Subtitle: the relative date. When the day has alerts, it also shows " • N Alert(s)".
  - Trailing: two stacked numbers, resting on top and high below. A warning icon is
    added when alerts > 0.
  - The ViewModel fills every day from start to min(end, today). Days without data are
    included with empty values (the filler lambda did not decompile, so this is inferred).
- **1y:** one row per week, newest first.
  - Title: the week range (start to start+6).
  - Subtitle: "N Alert(s)" when there are alerts.
  - Trailing: the same resting/high pair.

### Extra sections and empty states

- Empty states:
  - 1d: when the timeline has no heart-rate samples (false), the card shows the
    "No Heart Rate data" text in place of the chart.
  - 7d / 4w: the same text is shown when the list is empty.
  - 1y: the same text is shown when there are no weekly values (inferred).
- Help screen, one line per topic:
  - Help (default): explains the day's resting and high values and what each means.
  - About Your Reading: how to read the timeline and the zone colours.
  - How We Measure HR: the optical wrist sensor and how often it samples.
  - How to Use Your Reading: what trends in resting HR can tell you.
  - Abnormal HR Alert: what the high/low alerts are and how the thresholds work.

### Data needed

- 1d:
  - From the timeline:
    -: startTimestampGMT, heartRateValues [ts, bpm], restingHeartRate,
      maxHeartRate, minHeartRate, abnormalHRValuesArray, abnormalHrThresholdValue,
      lowAbnormalHrThresholdValue.
    -: zone floors.
    - activitiesForDayDTO, autoDetectedEventDTO, wellnessActivitySummaries.
  - Delete: a server endpoint.
- 7d / 4w: a server endpoint. It returns a
  list of `qbm` (HeartRateSummaryDTO), each with `calendarDate` and
  `values{restingHR (alt avgRestingHR), wellnessMaxAvgHR, abnormalHRAlertCount}`.
- 1y: a server endpoint, which returns the same DTO.
  `calendarDate` is the week start.
- Device capability: whether any paired device supports abnormal-HR alerts. This gates the
  settings menu item.

## Stress

### Navigation

- Top bar title: "Stress", with a back arrow.
- Overflow menu:
  - Help.
  - "Relax reminders": only when a device that supports the setting is present. It opens the
    device setting for relax reminders. Otherwise the menu has only Help.
- Tabs: **1d, 7d, 4w, 1y**, shown in portrait only. The default is **1d**.
  - The launching card can pass a start date and a range tab.
    Choosing ONE_DAY at launch also arms the first-use tooltip.
- Drill-downs:
  - 7d / 4w: tapping a day row opens the **1d** tab on that day.
  - 1y: tapping a week row opens the **7d** tab on the page that contains that week (
    case 22).
- The ViewModel stores the average stress of the current page (1d =
  ) in the UI state. It is passed to Help as
  `EXTRAS_AVERAGE_STRESS_LEVEL`.

### Charts

**1d tab: "Daily Timeline" card**

The card title is "Daily Timeline" in portrait and the date in landscape. Under the title is
the press-and-hold / double-tap hint. The chart is a with config
and data.

- Data: `allDayStressDTO.stressValuesArray` as [timestamp, level] pairs, one sample every
  3 minutes.
- X axis: 0 to 1440 minutes. Each bar sits at its minute + 1.5.
- Y axis: -8 to 100.
- Bar colours by level:

  | Level | Meaning | Bar |
  |---|---|---|
  | -1 | Invalid / unmeasurable | Skipped (no bar) |
  | -2 | Active (moving) | Short bar down to -8, active colour role |
  | 0-25 | Rest | Rest colour role |
  | 26-50 | Low | Stress colour role |
  | 51-75 | Medium | Stress colour role |
  | 76-100 | High | Stress colour role |

  Low, medium and high share the stress colour role; the bar height shows the level.
- Limit lines at 0, 25, 50, 75 and 100, with level labels.
- Tooltip: the time range, the value and the level name.
- Legend: Stress, Rest, Active and Unmeasurable. Each entry is shown only when that
  kind of sample occurs.
- Timeline event icons on the x axis.

**7d / 4w tabs: "Daily Averages" card**

Chart `msa`, a line chart.

- One series, "Overall Stress", with dots. Colour role: stress.
- Y axis: 0 to 100. The line splits at gaps.
- X labels: first and last date only.
- Tooltip: the date and the value.

**1y tab: "Weekly Averages" card**

Chart, a line chart.

- X = days / 7, no dots. Labels are month names.
- Series label: "Avg Overall Stress".

### Summary numbers

- **1d (portrait only), above the chart:**
  -: a large average stress number for the day ("--" if
    missing), plus a thin stacked bar. The bar shows the shares of rest, low, medium
    and high time, in their colour roles.
  -: a one-line qualifier message. It comes from `stressQualifier` (enum):
    - values: CALM, BALANCED, STRESSFUL, VERY_STRESSFUL, each with an _AWAKE variant, and
      UNKNOWN;
    - wording is present tense for today and past tense for earlier days (a noon threshold is
      involved);
    - UNKNOWN shows either a "could not be measured" or a "still pending" message;
    - the exact branch logic is inferred.
  -: four duration stats, Rest, Low, Medium and High, formatted as h/min.
- **7d / 4w:** one stat under the card: the average, labelled "Avg Stress Level".
  - The value is the integer mean (truncated, not rounded) of the daily overallStressLevel
    values, excluding nulls and -1.
  - A value of 0 or less, or a missing value, shows "--".
- **1y:** the same stat, computed as the truncated mean of the weekly values.

### Lists

- **7d / 4w:** one row per day that has data, from the end date back to the start
  date. Days without data are omitted.
  - Title: the weekday.
  - Subtitle: the relative date.
  - Trailing: the overall stress value ("--" if negative), plus a small proportional
    stacked bar of the rest/low/medium/high durations.
- **1y:** the weekly list reversed, newest first.
  - Title: the week range.
  - Value: "N Avg", or "--" if missing.
- **1d:** no list.

### Extra sections and empty states

- 1d: when there are no stress samples, the card shows the "No Stress data" text in place of
  the chart.
- 7d / 4w / 1y: when there is no data the card shows an empty text (exact text inferred to be
  the same shared no-data text).
- Help (stress help view), one line per topic:
  - Help (default): the period's average stress and what the number range means.
  - About Stress Levels: the four ranges and how stress relates to HR variability.
  - How to Use Your Reading: using rest periods and patterns to manage stress.
  - Stress Data Accuracy: why readings during movement are not counted.
  - Relax Reminders: what the device prompt does and when it triggers.

### Data needed

- 1d:
  - From the timeline:
    -: startTimestampGMT, stressValuesArray [ts, level].
    - `userDailySummary`: averageStressLevel, stressQualifier, restStressDuration,
      lowStressDuration, mediumStressDuration, highStressDuration.
    - activitiesForDayDTO and the other event lists (for the event icons).
- 7d / 4w: a server endpoint. It returns a
  list of `osa`, each with `calendarDate` and `values{overallStressLevel, restStressDuration,
  lowStressDuration, mediumStressDuration, highStressDuration}`. Gaps are not filled.
- 1y: a server endpoint. It returns a list of
  , each with `calendarDate` (week start) and `value`.
- Device capability for relax reminders.

## Body Battery

### Navigation

- Top bar (portrait only): title "Body Battery", with a back arrow. The overflow menu has only
  Help.
- Tabs: **1d, 7d, 4w only.** There is no 1y tab. Tabs are shown in portrait
  only, and the default is **1d**.
  - The launching card can pass a start date and a range tab.
- An optional banner slot sits above the tabs (from case 5). It looks like the
  shared sleep-data notice: a missing-sleep-data or sleep-education message with a "Learn
  more" link (inferred). Low priority.
- Drill-down: 7d / 4w day row -> **1d** tab on that day.
- 1d: tapping a factor row opens a bottom sheet with the details of that event.
- Help passes these extras:
  - a launch flag (UiState `isSummarySelected`);
  - a launch flag;
  - a launch flag (the highest bodyBatteryVersion seen, default 1.0);
  - a launch flag.

### Charts

**1d tab: "Daily Timeline" card**

The card title is "Daily Timeline" in portrait and the date in landscape. The card is about
350 dp tall, with the hint text under the title. The chart is a 
with config, made of a line layer and two bar layers (in overlay mode). The
chart can be zoomed into a time interval (container, zoom state `jdf`).

- **Overlay chips** in the card footer: a multi-select chip row with Body Battery, Stress and
  Rest (enum).
  - A chip is shown only when its data exists:
    - Body Battery: any BB sample has a value;
    - Stress: any stress sample is 26-100, or any sample is Active (-2);
    - Rest: any stress sample is 0-25.
  - The selection is saved across sessions. merges the new choice with saved
    choices for chips that are not shown.
- **Body Battery line**:
  - Data: `allDayStressDTO.bodyBatteryValuesArray`. Each entry holds a timestamp, a status, a
    level (0-100) and a version (entry `sj1$a`: time, status, value, version).
    - Status values: MEASURED, MODELED, ADJUSTED, ADJUSTED_MODELED, INVALID, RESET.
    - Samples are every 3 minutes. Each point is plotted at its minute + 1.5.
  - Segments:
    - MEASURED and ADJUSTED points form a solid, lightly smoothed line with a fill below it.
    - MODELED, ADJUSTED_MODELED and RESET points form a dashed line with a fill below it.
    - A gap of 3 to 180 minutes between points is bridged by a dashed segment.
    - A gap over 180 minutes breaks the line.
  - Line colour role: body-battery line. The fill is a vertical gradient from a body-battery
    tint at the top to transparent.
  - Y axis: -8 to 100. The -8 band leaves room for Active stress bars.
  - X axis: 0 to 1440 minutes. When zoomed, the axis spans the zoom interval, labelled at the
    start, the end and the quarter points.
- **Stress overlay**: stress bars for 26-100 and Active bars for -2, using the same rules as
  the Stress 1d chart, drawn in overlay mode behind the line. The limit lines come from the
  stress config.
- **Rest overlay**: rest bars for 0-25.
- Tooltip on the BB line:
  - the time range (3 minutes);
  - the value;
  - the label "Body Battery";
  - a level label, shown unless the sample's version is 3:

    | Level | Label |
    |---|---|
    | 0-25 | Very Low |
    | 26-50 | Low |
    | 51-75 | Medium |
    | 76-100 | High |

     also defines an alternative band set: 0-29, 30-59, 60-84, 85-100. Where it is used
    is Unknown (not traced).
- Legend:
  - "Body Battery" (solid);
  - "Estimated" (dashed), when any modeled, adjusted-modeled or reset point exists;
  - the stress legend entries for the overlays that are on (Unmeasurable, Stress, Rest,
    Active).
- Timeline event icons on the x axis, shown only when not zoomed.

**7d / 4w tabs: "Daily Values" card**

Chart `lta`, a with config. It is a floating range chart, one per day.

- For each day, a thick vertical stroke (width 8, bar colour role) runs from the day's low to
  its high.
- A dot marks the top end (high colour role) and another the bottom end (low colour role).
- Y axis: 0 to 100. A small x padding is added at both ends.
- Legend: "Daily High" and "Daily Low".
- Tooltip: the date (relative format), the value, and "Highest" or "Lowest".
- Empty text: "No data available for this range" when the list is empty.

### Summary numbers

**1d (portrait only), above the chart**

- Gauge (176 dp ring):
  - Today: the ring is split into four equal quarters and filled to
    `bodyBatteryMostRecentValue / 100`. The centre shows the current value ("--" if 0 or
    less) over "100".
  - Past day: the ring is one full segment. It uses the accent colour role when both
    values exist, otherwise a neutral role. The centre shows the day's highest value labelled
    "High" and its lowest value labelled "Low", with "--" for missing values.
- Status headline and message, below the gauge:
  - If `bodyBatteryVersion` is 3 and the day has a `bodyBatteryDynamicFeedbackEvent`:
    - the headline, body and an optional "learn more" style link come from the feedback
      codes;
    - the mapping is in (about 5000 instructions, not decompiled), so it is Unknown.
  - Otherwise, when the day has no charged, drained, highest or lowest value, a no-data
    headline is shown.
  - Otherwise, for **today**, a message comes from a local catalogue. It is chosen by:
    - time of day;
    - the current BB level band;
    - whether sleep has been confirmed (`confirmedTotalSleepSeconds` present).
    - It is prefixed with a change phrase built from `messagingToday.deltaValue` (gained N,
      no change, or lost N).
    - When several messages match, the app rotates through them once per minute.
  - Otherwise, for a **past day**:
    - if the valid BB samples cover less than 12 hours (count x 3 min), a message says there
      was not enough data for the day;
    - else an end-of-day message is picked by the last BB value and the drained amount
      (inferred: the range test uses getter g = drained).
  - Nimrag cannot reuse the catalogue texts. Treat these as "status message" with our own
    wording.
- Stat row, two stats:
  - "Charged" = `+bodyBatteryChargedValue`;
  - "Drained" = `-bodyBatteryDrainedValue`;
  - "--" when missing.

**7d / 4w**

- No aggregate numbers. Only the chart and the list.

### Lists

- **1d "Factors" section**: portrait only, and only when there is at least one event
  with a known type.
  - Events come from a server endpoint
   . They are sorted by `eventStartTimeGmt`, newest first.
  - Row left side:
    - title:

      | Event type | Title |
      |---|---|
      | ACTIVITY | the activity name |
      | SLEEP | "Sleep" |
      | NAP | "Nap" |
      | STRESS | "High Stress" or "Low Stress" (inferred split) |
      | RECOVERY | Unknown (not traced) |

    - subtitle: the duration, from `durationInMilliseconds`. For SLEEP the duration is
      replaced by the timeline's sleep total when that is > 0;
    - a marker when the event has feedback (`shortFeedback` not NONE, or a `feedbackType`).
  - Row right side: the impact `bodyBatteryImpact` as "+N" or "-N", with an up, down or
    neutral arrow icon. The impact is hidden for NAP or RECOVERY with impact 0 or less, and
    for STRESS with impact 0 or more.
  - Row tap opens the event bottom sheet, which contains:
    - the event title and time;
    - a feedback sentence chosen from the event's feedback code (benefit/maintenance/stress
      themes; texts not copied);
    - an event-window chart (`in` lambda: BB values and stress values from the event DTO's
      arrays);
    - more stats, not fully traced (Unknown).
- **7d / 4w**: one row for every day in the range, newest first, including days
  without data.
  - Title: the weekday.
  - Subtitle: the relative date.
  - Trailing: two values, `highBodyBattery` labelled "High" and `lowBodyBattery` labelled
    "Low". Values of 0 or less show "--".
  - Tap opens the 1d tab.

### Extra sections and empty states

- 1d: a has-data flag is true when the day has BB samples and any of charged,
  drained, highest or lowest is 0 or more. It drives the chart container's empty state. The
  exact empty text is Unknown (inside, not traced).
- 7d / 4w: "No data available for this range" in the card.
- Help, one line per topic:
  - Default: an overview linked to the current day and the BB version.
  - What is Body Battery: the energy-gauge idea, rated 0-100.
  - Readings: how to read the level bands and the chart.
  - How we calculate it: the inputs (HRV, stress, sleep, activity).
  - Estimated Body Battery: why some parts are drawn dashed (modeled data).
  - Merged data: shown when the "true up" flag is set, about merging data from several
    devices.
  - Compatible devices: a link out to a support page.

### Data needed

- 1d (= messagingToday + timeline + events):
  - Timeline:
    -: calendarDate, startTimestampGMT, endTimestampGMT,
      stressValuesArray, bodyBatteryValuesArray [ts, status, level, version];
    - `userDailySummary`: bodyBatteryChargedValue, bodyBatteryDrainedValue,
      bodyBatteryHighestValue, bodyBatteryLowestValue, bodyBatteryMostRecentValue,
      bodyBatteryVersion, bodyBatteryDynamicFeedbackEvent {eventTimestampGmt,
      bodyBatteryLevel, feedbackShortType, feedbackLongType}, bodyBatteryActivityEventList;
    - the sleep total from the sleep DTO;
    - activities and events, for the icons.
  - a server endpoint returns a list of
    . Each has:
    - an event: eventType (ACTIVITY, NAP, RECOVERY, SLEEP, STRESS), eventStartTimeGmt,
      timezoneOffset, durationInMilliseconds, bodyBatteryImpact, feedbackType, shortFeedback,
      moveIQActivityType/SubType;
    - activityName, activityType, activityId, averageStress;
    - stress and BB value descriptors and value arrays.
  - a server endpoint, fetched only when the date is
    today. Fields: calendarDate, start/end timestamps (GMT and local), deltaValue,
    timeOfDay, confirmedTotalSleepSeconds, bodyBatteryVersion.
- 7d / 4w: a server endpoint. It returns
  a list of, each with `calendarDate` and `values{highBodyBattery (alt
  avgDailyHighBodyBattery), lowBodyBattery (alt avgDailyLowBodyBattery)}`.
- Local preference: the selected overlay set.

## Steps

### Navigation
- Title: Steps. Tabs: 1d, 7d, 4w, 1y. The default is 1d unless the caller passes a starting tab and date.
- Overflow menu: Personal Records, Edit Goal and Help. Edit Goal appears only when the page date is today (inferred from the availability flag).
- In portrait, the tab row and period header sit above the pager. In landscape, both are hidden and the 1d chart fills the screen. Rotating by sensor is allowed on the 1d tab (inferred).
- The 1y page has its own two-way toggle: Weekly totals and Monthly totals. The choice is saved on the device and restored next time. The default is Weekly.

### Charts
**1d**
- Ring gauge: today's steps against the daily step goal.
  - No data: the whole ring is in the track colour.
  - Steps at or above the goal: the whole ring is in the goal-reached colour.
  - Otherwise: a progress arc plus the remainder in the track colour.
- Daily timeline (line/area chart):
  - The line shows cumulative steps across the day. X runs over minutes 0 to 1440. Each point is a 15-minute interval, so a day has at most 96 points.
  - The line breaks over intervals whose activity level is "none". The area under the line has a gradient fill in the metric colour.
  - The goal is a dashed horizontal line in the metric colour. A small check-mark marker sits where the cumulative line crosses the goal; the crossing point is interpolated.
  - Y range: 0 up to max(last cumulative value, goal). The fallback maximum is 10,000.
  - The legend shows "Goal" when a goal exists.
  - An overlay strip marks activities and sleep from the day's timeline. Some event kinds are left out.
  - Interaction: press-and-hold in portrait, double-tap in landscape. Scrubbing shows a tooltip with the time and the cumulative steps.
  - The chart title is "Daily timeline" in portrait and the date in landscape.

**7d / 4w**
- Bar chart titled "Daily totals", one bar per day.
  - Bar colour: goal reached when that day's steps are at or above that day's goal, otherwise in progress.
  - Each bar carries a goal marker at that day's goal value. A bar shadow in the track colour shows the empty column.
  - Y max = max(largest day, largest goal). The fallback is 10,000.
  - The tooltip shows the date and the steps.

**1y, Weekly totals**
- 52 bars in a single colour, one per week. There is no goal colouring.
- The fallback y max is 56,000.
- The tooltip shows the week's date range and the step total.

**1y, Monthly totals**
- 12 bars, one per month. The fallback y max is 240,000.
- The tooltip shows the month and the total.

### Summary numbers
**1d**
- Ring centre: the step count. The exact centre text is Unknown (the gauge text helper was not inspected).
- Percent of goal = steps * 100 / goal, using integer division.
- Achievement chip, shown when one applies. Priority order:
  1. "New PR": the day holds a personal record and the day is today.
  2. "PR": the day holds a personal record.
  3. "N-day streak": the current goal-streak record is at least 2, the day is today, and the goal is met.
  4. "Goal met": percent / 100 equals 1.
  5. "N x goal": percent / 100 is greater than 1.
- Two tiles:
  - Distance: wellness distance in metres, shown in km or mi per the unit setting.
  - Calories: the day's total kilocalories as an integer.

**7d / 4w footer**
- Row 1: Total steps, the sum of daily totals ("--" when none), and Total distance (km or mi).
- Row 2: Avg daily, the server-side daily average. Avg weekly, the server-side weekly average, is shown only on 4w and left blank on 7d.

**1y footer**
- Total steps: the sum, shown as empty when it is 0. Total distance.
- Avg daily = total / sum of the per-period counts of days with data.
- Avg weekly (Weekly mode) or Avg monthly (Monthly mode) = total / number of periods that have steps above 0.

### Lists
- **7d / 4w**: a "Personal Records" link row, then one row per day, newest first.
  - Title: the weekday. Subtitle: the date and the percent of goal.
  - Trailing: the step count with a small progress indicator.
  - Tap opens the 1d tab at that date.
- **1y Weekly**: a Personal Records link, then one row per week, newest first. Each row shows the week range and its total. Tap opens the 7d tab for that week. The exact anchor date is Unknown (likely the week's last day).
- **1y Monthly**: a Personal Records link, then one row per month, newest first. Rows are not tappable.

### Extra sections and empty states
- Goal editing (separate screen):
  - Choice between an automatic goal, which the device adjusts daily, and a manual goal.
  - A manual goal is 1 to 999,999 steps. The input dialog starts at 7,500.
  - The goal is saved as a permanent goal.
- Help opens one help topic about steps. Its contents were not read.
- Empty states:
  - 1d chart: "No steps data" when both steps and goal are missing. A separate flag covers a day that has a total but no 15-minute data. In that case the chart is empty while the ring and tiles still show the total (inferred).
  - 7d/4w chart: empty with the no-data text when no day has steps above 0.
  - Missing days are always present as empty entries; the window is filled day by day. Their bars are absent and their list values show "--".

### Data needed
- 1d:
  - a server endpoint: totalSteps, dailyStepGoal, wellnessDistanceMeters, totalKilocalories, wellnessStartTimeGmt.
  - a server endpoint: list of {startGMT, endGMT, steps, primaryActivityLevel, activityLevelConstant}.
  - a server endpoint: activities and sleep for the overlay.
  - Personal records filtered to steps type ids 12 and 16. Id 16 is the current goal streak and counts only while it is current.
- 7d/4w: a server endpoint.
  - values[] {calendarDate, values: {totalSteps, totalDistance, stepGoal}}.
  - aggregations {totalStepsAverage, totalStepsWeeklyAverage}.
- 1y weekly: a server endpoint returns {calendarDate, values: {totalSteps, totalDistance, wellnessDataDaysCount}}.
- 1y monthly: a server endpoint returns the same shape per month.
- Goal: GET and PUT a server endpoint. That this is the endpoint the edit screen uses is inferred.

## Floors

### Navigation
- Title: Floors. Tabs: 1d, 7d, 4w, 1y. The default is 1d.
- Overflow menu: Edit Goal only. It is visible in portrait when goal editing is available (today; inferred). No help entry was found.
- In portrait, the tab row and header sit above the pager. Landscape hides them. Sensor rotation and auto-rotate flags exist.

### Charts
**1d**
- Ring gauge: floors climbed (whole floors) against the floors goal. It uses the same three ring states as Steps.
- Daily timeline, a mirrored area chart:
  - Cumulative floors climbed are drawn above the zero line and cumulative floors descended below it (as negative values). X runs over minutes 0 to 1440.
  - Points come from 15-minute buckets. A segment breaks whenever two consecutive points are not exactly 15 minutes apart. Both areas use a gradient fill.
  - The goal is a dashed horizontal line in the goal colour, shown only when climbed or descended data exist. A check marker sits at the crossing point.
  - Y range is symmetric: plus and minus a rounded max(last climbed, last descended, goal). The fallback is 20.
  - The activity and sleep overlay strip is the same as Steps.
  - Tooltip: the time, plus the climbed value with an up arrow and the descended value with a down arrow.
  - Legend: Climbed, Descended, Goal.

**7d / 4w**
- Bar chart "Daily totals" with two bars per day: climbed above zero and descended below zero.
  - A zero line is drawn.
  - A single dashed goal line uses the goal of the most recent day that has one.
  - Y max = max(largest climbed, largest descended, goal), rounded up to an even number. The fallback is 16.
  - Tooltip: the date and the value.

**1y**
- Bar chart "Weekly totals", 52 weeks, climbed up and descended down. No goal line.

### Summary numbers
- 1d:
  - Ring label: floors climbed.
  - Tiles: Climbed and Descended, floored to whole numbers ("--" when missing).
  - Goal: the day summary's floors goal, falling back to the effective wellness goal.
- 7d/4w footer, two rows:
  - Total climbed + Daily avg climbed.
  - Total descended + Daily avg descended.
  - Average = total / number of days whose value exists and is at least 0, rounded to the nearest whole number.
- 1y footer: Avg weekly climbed and Avg weekly descended = sum / number of weeks whose value exists and is at least 0.

### Lists
- 7d/4w: every day in the window, newest first, including days with no data.
  - Each row shows the weekday and date, climbed with an up arrow ("--" when missing), and a small ring of climbed against the goal.
  - Tap opens 1d for that day.
- 1y: only weeks with at least one data day, newest first. Each row shows the week range and the climbed value. Tap opens 7d for that week.

### Extra sections and empty states
- Goal editing (separate screen):
  - Integer from 0 to 999.
  - A "Use default" action restores the default goal.
  - Saved as goal type FLOORS_CLIMBED.
- Empty states:
  - 1d with no data: the ring is in the track colour, the tiles show "--", and the chart has no series and no goal line.
  - Missing days stay in the 7d/4w list as "--". Missing weeks are dropped from the 1y list.

### Data needed
- 1d:
  - a server endpoint: startTimestamp and endTimestamp, plus floorValuesArray[] of {startDateTime, endDateTime, ascended, descended}. Cumulative sums are computed in the client.
  - Snapshot timeline for the date, from userDailySummary: floorsAscended, floorsDescended, userFloorsAscendedGoal.
  - a server endpoint.
- 7d/4w: a server endpoint returns {calendarDate, values: {wellnessFloorsAscended, wellnessFloorsDescended, wellnessUserFloorsAscendedGoal}}.
- 1y: a server endpoint returns {calendarDate, values: {wellnessFloorsAscended, wellnessFloorsDescended, wellnessDataDaysCount}}.
- Goal save: the wellness-goals floors-climbed resource with {goalType: FLOORS_CLIMBED, value}.

## Calories

### Navigation
- Title: Calories burned. Tabs: 1d, 7d, 4w, 1y. The default is 1d unless the caller passes one.
- Overflow menu: Help only. No goal editing.
- Layout, top to bottom:
  - An optional promo banner, served by the shared promos provider and probably tied to nutrition (inferred from the nutrition-enabled flag in the state).
  - The tab row.
  - The period header.
  - The pager.
- Changing tabs also updates paging bounds (inferred).

### Charts
**1d**
- Donut ring split by source:
  - A resting segment, sized as resting / total, in the resting role colour.
  - An active segment, sized as active / total, in the active role colour.
  - With no total, the whole ring is in the track colour.
- The centre shows the day total labelled "Total". There is no time-of-day chart.

**7d / 4w**
- Bar chart "Daily totals", one column per day, built from two overlapping bars:
  - A total bar in the active colour.
  - A resting bar drawn over it from zero, in the resting colour.
  - Visually, resting sits at the bottom and active is the part above it.
  - When total equals resting, the resting bar takes the highlight role.
- X covers every day in the window. Bar width differs between 7d and 4w.
- Y max = the largest value, rounded up to the next 10 (below 100) or the next 100. The fallback is 3,000.
- The legend uses the resting colour.
- Tooltip: the day's values, built from the day's row data. Exact fields are Unknown; likely total, active and resting.
- Empty: the no-calories-data text when no bars exist.

**1y**
- Bar chart "Daily averages", one column per week, with the same two-bar layering.
  - Each value is that week's daily average: weekly total / data-day count and weekly resting / data-day count.
  - Weeks with zero data days are skipped.
  - Y scaling and the fallback (3,000) are the same as 7d/4w.

### Summary numbers
- 1d:
  - Total, plus two tiles: Active (active colour) and Resting (resting colour).
  - Each is a plain integer with a thousands separator. 0 or missing shows "--".
  - A small footnote states that total is active plus resting.
- 7d/4w footer:
  - Average active and Average resting tiles.
  - One line with the average total.
  - Values come from the server aggregations activeCaloriesAverage, restingCaloriesAverage and totalCaloriesAverage, truncated to integers. 0 shows "--".
- 1y footer: the same three averages, computed in the client.
  - Total days = sum of the weeks' data-day counts.
  - Avg active = sum of active / total days. Avg resting = sum of resting / total days. Avg total = sum of total / total days. All use integer division.
  - When no week has the field, the value is "--".

### Lists
- 7d/4w: one row per day, newest first; missing days are filled as empty rows.
  - Each row shows the weekday, the date and the value "N Total" ("--" when missing).
  - Tap opens 1d for that day.
- 1y: one row per week, newest first, including weeks filled as empty.
  - Each row shows the week range (start to start + 6) and the week's average daily total with an "Avg daily" label.
  - Tap opens 7d for that week.

### Extra sections and empty states
- Help explains how total, active and resting relate. It is given the numbers of the page being viewed:
  - 1d: the day's values, marked per day.
  - 7d/4w: the server averages.
  - 1y: the client averages.
- No goal on this screen.
- Missing days and weeks appear as "--" rows. The charts simply omit their bars.

### Data needed
- 1d, 7d and 4w all use a server endpoint. For 1d, start and end are the same day and only the first value is used.
  - values[] {calendarDate, values: {totalCalories, activeCalories, restingCalories}}.
  - aggregations {totalCaloriesAverage, activeCaloriesAverage, restingCaloriesAverage, caloriesAverage}.
- 1y: a server endpoint returns [{calendarDate, values: {totalCalories, activeCalories, restingCalories, averageDailyTotalCalories, wellnessDataDaysCount}}].

## Intensity Minutes

### Navigation
- Title: Intensity Minutes. Tabs: 1d, 7d, 4w, 1y. The default is 1d. The caller may pass a starting tab and date.
- Overflow menu: Help always, and Edit Goal only when the page is in the current calendar week.
  - "Current week" is checked only on the 1d and 7d tabs. It means the page window lies inside the current week.
  - Weeks run Monday to Sunday.
- The 7d tab follows calendar weeks rather than a rolling window (inferred from the current-week test and the week-based endpoint).
- The other tabs use the shared tab row, period header and pager.

### Weighting and goal logic (applies everywhere)
- Credited minutes = moderate + 2 * vigorous.
  - Negative or missing parts count as 0.
  - When a DTO carries both a "value" and a "minutes" field for moderate or vigorous, the value field wins.
- The goal is weekly. Its default is 150 when the server sends none.
  - On multi-day data, the goal used is the first positive weeklyGoal in the list.
  - On 1d, it is weekGoal from the daily DTO.
- Viewing the 1d page for today stores that weekly goal in the screen state.
- Every load of 7d, 4w or 1y data finds the entries whose week contains today. It sums their moderate and vigorous minutes into a current-week summary used by Help (inferred use).

### Charts
**1d**
- Line chart, the weekly running total traced through the day:
  - It starts at the week-to-date total at the start of the day (startDayMinutes).
  - Each 15-minute sample in imValues is added to it.
  - It ends at the week-to-date total at the end of the day (endDayMinutes).
- When the gap between samples is more than one step, the previous value is carried forward, giving a flat line.
- The weekly goal is a dashed horizontal series in the metric colour. Legend: "Weekly goal".
- Y range:
  - Minimum = min(goal, start value) * 0.95, floored to a multiple of 5 and never below 0.
  - Maximum = max(goal, end value) * 1.05, at least 150 and rounded up to a multiple of 5.
  - The maximum is then aligned so the three labels fall on even steps (inferred).
- X axis: the day in 15-minute steps, with labels every 4 hours (12h or 24h per the system setting).
- Activity icons are drawn on the x axis at the start times of activities and Move IQ events that earned minutes. A sleep and nap band also shows.
- Scrubbing shows a tooltip with the 15-minute time range and the running total.
- The card is titled "Daily timeline". Its header shows the summary numbers listed below.

**7d (calendar week)**
- Line chart "Weekly timeline", a running total from 0 across the week:
  - With hourly samples, each sample is added.
  - With no hourly samples for a day, that day's credited total is added once, at noon.
- Dashed weekly-goal line. The y range follows the 1d rules with a start of 0. Activity icons are not drawn.

**4w**
- Line chart "Daily totals" built from day totals.
  - Days are grouped by calendar week.
  - Each week is its own running series that starts at 0 and adds each day's credited minutes, plotted at the start of the day.
  - Each week has its own goal line, taken from that week's first positive goal or 150.
- X covers 28 days.

**1y**
- Bar chart "Weekly totals", one bar per week across 52 weeks.
  - Bar value = credited minutes for the week.
  - Colour role: goal reached when the value is at or above that week's goal, otherwise in progress.
  - Each bar carries its week's goal marker. A bar shadow in the track colour is shown.
- Y max = the ceiling of max(largest value, largest goal). The goal falls back to 150.
- Tooltip: the week range, the value and the metric name.

### Summary numbers
- 1d header:
  - A Moderate tile (moderate minutes).
  - A Vigorous tile (vigorous minutes, with an accessibility note that it counts double).
  - A Total line (moderate + 2 * vigorous) for the day.
- 7d, current week:
  - A ring gauge of week total against the weekly goal: progress fraction plus track, or the whole ring in the goal colour when met.
  - A centre value with a percent-of-goal line.
  - The text "Weekly goal met" once reached.
  - This block shows even with no data, starting at 0.
- 7d, past weeks:
  - The same gauge.
  - A chip when the goal was met. When the total is two or more times the goal, the chip carries an "N x" prefix (N = total / goal, integer division).
  - Below the chart: Moderate and Vigorous totals for the week. These are hidden when both are 0.
- 4w footer:
  - Avg moderate and Avg vigorous: the sum over the days up to today, divided by the number of weeks those days span (rounded up to whole weeks).
  - Avg daily total: credited minutes per included day.
  - Exact tile labels on this tab are Unknown (the label lambda sits in an unread dex).
- 1y footer:
  - Avg weekly moderate and Avg weekly vigorous, each the mean over weeks that started before the current week.
  - Avg daily total is not confirmed for this tab.
- A separate "what it takes" panel exists. It shows the remaining minutes as moderate, or half that amount (rounded up) as vigorous. Where it appears is Unknown.

### Lists
- 1d: section "Activity Intensity Minutes", listing the day's activities and Move IQ events that carry intensity minutes (inferred filter).
  - Title: the activity name. Move IQ events get a "Move IQ" suffix on their activity type.
  - Subtitle: the start time, then vigorous and moderate minutes.
  - Trailing: the credited minutes (moderate + 2 * vigorous) with a minutes unit.
  - Tap opens the activity detail, or the Move IQ event view for Move IQ items.
- 7d and 4w: one row per day, newest first.
  - Each row shows the weekday, the date and the credited minutes ("--" when missing or negative).
  - Tap opens 1d for that day.
  - On 7d, the list shows only when the week has data.
- 1y: one row per week, newest first.
  - Title: the week range (start to start + 6). Value: the credited minutes. Subtitle: percent of goal. A small progress indicator.
  - Tap opens 7d for that week.

### Extra sections and empty states
- Edit Goal dialog:
  - A numeric field for the weekly goal.
  - Accepted range: 150 to 999,999. Out-of-range input shows a short range-error message and does not save.
  - Saved as wellness goal type INTENSITY_MINUTES on the "intensity-minutes" goal resource. The data is then refreshed (inferred).
- Help opens one help topic, passing the current-week totals.
- Empty states:
  - 1d: when the day DTO is missing, or has no end timestamp, a plain empty card replaces the chart and list.
  - 7d past week, 4w and 1y: the chart area becomes an empty card when no entry has moderate or vigorous data.
- Missing days and weeks are filled as empty entries by the window filler (one-day or seven-day step). They show as "--" rows.

### Data needed
- 1d: a server endpoint returns:
  - calendarDate, startTimestampGMT, endTimestampGMT, startTimestampLocal, endTimestampLocal.
  - weekGoal, weeklyTotal, dayOfGoalMet, startDayMinutes, endDayMinutes, moderateMinutes, vigorousMinutes.
  - imValues[] {timestamp, value}.
  - sleepTimes, activities[], naps[].
- 7d: a server endpoint with days = 7. It returns {weeklyGoal, dailyStats[]}, where each entry is the summary shape below with hourlyValues[] {timestamp, value}.
- 4w: a server endpoint.
- 1y: a server endpoint.
- Summary shape for 7d, 4w and 1y: {calendarDate, weeklyGoal, moderateValue, vigorousValue, moderateMinutes, vigorousMinutes, timezoneOffset, startTimestampGMT, endTimestampGMT, hourlyValues[]}.
- Goal:
  - Read with a server endpoint.
  - Save {goalType: INTENSITY_MINUTES, value} through the wellness-goals repository. The exact write path was not traced.

## Sleep

This is the detail screen that opens when the Sleep card on My Day is tapped. A single activity hosts it. It always has four period tabs. The 1-day tab is a rich per-night view. The 7-day and 4-week tabs show per-night trend charts plus a list. The 1-year tab shows per-week trend charts plus a list. Several sections only appear when the user's devices report a matching capability (see "Capability gates" under Data needed).

### Navigation

- **Entry and arguments.** The caller can pass a start date (default: today), a starting period tab (default: 1 day), a sub-tab of the day view (Score or Coach, default Score), a preselected timeline overlay, and a section to scroll to. It can also pass a flag that resets the breathing-variations education. If no overlay is passed, the last overlay the user picked is restored from local preferences.
- **Title.** The title is "Sleep Score" when the user has a sleep-score-capable device and "Sleep" otherwise (inferred: the choice follows the capability helpers). There is a back arrow.
- **Overflow menu.**
  - "Help" opens a help screen. Its topics are: overview, sleep score, sleep coach, timeline metrics, sleep alignment, sleep stages, sleep tracking and data accuracy.
  - "Edit Sleep Times" appears only when editing is available for the selected night, and it is enabled only on the 1-day tab. It opens a bottom sheet with a start and end time and a "Save" action. The same sheet is titled "Add Sleep Times" when no sleep exists for that night.
- **Promo banner.** An optional promo banner from the promotions system (sleep placement) sits at the very top.
- **Period tabs.** A fixed tab row: `1 Day | 7 Days | 4 Weeks | 1 Year`. The default is 1 Day, unless the caller passes another.
- **Period header.** Below the tabs there is a prev/next header with left and right arrows around a date label.
  - On 1 Day, the label is the single date, and the arrows move one day.
  - On 7 Days and 4 Weeks, the label is a date range ending on the selected date, and the arrows move the window by its own length.
  - On 1 Year, the label is a range of 52 weeks (approximately), and the arrows move by a year.
  - Forward navigation stops at today (inferred: this is shared report-pager behaviour).
- **Swiping.** The content is a horizontal pager, so swiping left or right also moves the period.
- **Day sub-tabs.** These appear only when the user has a sleep-coach-capable device. A segmented control `Sleep Score | Sleep Coach` drives a two-page pager inside the 1-day tab. Without coach capability, only the score page is shown. Without score capability either, a simpler legacy day page is shown (inferred: duration plus stages only).
- **Cross-navigation from lists.**
  - Tapping a night row on the 7-day or 4-week tab switches to the 1-day tab on that date.
  - Tapping a week row on the 1-year tab switches to the 7-day tab on the week that starts on that date.
- **Onboarding overlays.** First-time overlays explain skin temperature and breathing variations. A tooltip also points to the arrow that expands the score factors. Each of these is shown once and then dismissed for good (stored in prefs).

### Charts

**1 Day, Score page**

1. **Stages / Timeline switch.** A segmented control `Timeline | Stages` sits under the header block. The user's last choice is remembered.
2. **Stages chart.** A ring/donut chart about 185dp across with a thin ring.
   - It has four arcs for deep, light, REM and awake. Each arc is that stage's share of the total stage time, as a percentage.
   - Each stage uses its own stage colour role: deep, light, REM, awake.
   - If the device is not REM-capable, the REM arc is zero.
   - If there is no stage breakdown, a single full ring in the light-stage role is drawn.
   - If the night is manual or off-wrist, a full ring in a neutral/disabled role is drawn.
   - The centre shows the total sleep time (inferred).
   - A legend lists each stage with its duration and percentage (inferred: the legend composable could not be fully decoded).
3. **Timeline chart.** This is a hypnogram over the sleep window.
   - The x axis is local clock time from sleep start to sleep end.
   - The rows are the stage levels (deep, light, REM, awake), each coloured with its stage role.
   - Below the hypnogram is a strip of overlay chips. One overlay series is drawn at a time, aligned to the same time axis.
   - **Overlay options, in order,** each shown only if the device supports it:
     - Breathing Variations
     - Movement (or, for sleep version 2, Awake/Restlessness)
     - Resting HR
     - Body Battery
     - Skin Temp
     - Pulse Ox
     - Respiration (or, for respiration version 200, Respiration Averages)
     - Overnight HRV
     - Overnight HR, which is defined but rarely used
   - Each overlay has its own empty text when its series is missing, for example no restless-moments data, no respiration data or no overnight HRV data.
   - Scrubbing shows a crosshair with the time and the value at that point (inferred: this is the shared MPAndroidChart highlight pattern).

**1 Day, Coach page** (coach-capable devices only)

- There are no plotted charts. It is a card layout; see Extra sections.

**7 Days and 4 Weeks** (one point or bar per night, x axis = calendar dates in the window)

1. **"Sleep Score" trend.** A line chart of the nightly overall score.
   - The y axis is fixed at 0–100.
   - Consecutive nights are joined by a line. A night with no neighbour is drawn as a single dot.
   - A second series can be overlaid, chosen by chips. In order, the chips are: HRV Status, Respiration, Pulse Ox, Skin Temp Change, Body Battery (change), Overnight HR, and Resting HR. Each chip is shown only if the user's device supports it.
     - Body Battery change is drawn with square markers on its own 0–100 axis.
     - The other overlays use their natural units on a secondary axis (inferred).
   - The score line uses the score role colour. The overlay uses the overlay's metric role.
   - On tap or scrub, a tooltip shows the date, "Score" and the value, plus the overlay value.
   - The whole chart is hidden when the user has no score-capable device (inferred from the gating used in the list below).
2. **"Sleep Duration vs. Sleep Need" chart** (titled "Sleep Duration" without coach capability). A bar chart, about 350dp tall.
   - There is one bar per night, equal to total sleep time in hours.
   - The night's sleep need is drawn as a taller, muted "track" bar behind it. Without coach capability, an 8-hour target is used instead.
   - The bar uses the "goal met" role when duration ≥ need, and otherwise an "in progress" role.
   - The y max is max(duration, need) rounded up to a multiple of 4 hours. Ticks are in hours.
   - Legend: Sleep Duration, Sleep Need, and Need Met (or Duration Met).
   - The tooltip shows the date and the duration (h m).
   - The empty text says there is no duration data.
3. **"Sleep Consistency" chart** (on a two-page switcher `Sleep Consistency | Sleep Alignment` when the device supports alignment).
   - There is one vertical segment per night, running from bedtime to wake time. The y axis is time of day, inverted so that evening is at the top.
   - The y range is rounded to whole hours and padded to a multiple of 4 hours.
   - Two horizontal reference lines mark the average bedtime and the average wake time.
   - Segment colour: the consistency role, or the "goal met" role when that night's alignment status is "aligned".
   - Segments are drawn thicker when the window is short (7 days) and thinner for 4 weeks.
   - Legend: Sleep Times, Avg Bedtime, Avg Wake Time, and Rhythm Aligned (when alignment is available).
   - The tooltip shows the date, the bedtime, the wake time and an alignment status word: Aligned, Ahead, Behind, Irregular, or Not aligned.
   - The empty text says there is no consistency data.
   - Sleep Alignment page: per-night alignment against the optimal window. Unknown — the exact chart form was not decoded. It is likely the same segment chart plotted against the optimal-window band from the alignment fields.

**1 Year** (one point or bar per week)

1. **"Sleep Score" weekly chart** (score-capable only). A line or point chart of the weekly average score on a 0–100 axis. The tooltip shows the week range and "Avg Score". The empty text says there is no score data.
2. **"Sleep Duration vs. Sleep Need" weekly chart** (titled "Sleep Duration" without coach capability). Bars show the weekly average sleep time against the weekly average need. It uses the same colour roles as the 7d/4w chart. Weeks with an average of zero or less are skipped.
3. **"Sleep Consistency" weekly chart.** Per-week segments from the average bedtime to the average wake time, with the same axis rules as the 7d/4w chart.

### Summary numbers

**1 Day header**

| Number | Meaning / computation | Unit, rounding | Qualifier |
|---|---|---|---|
| Score | The overall sleep score for the night, as an integer. | Shown "N / 100" with the label "Score" | The overall qualifier word (Excellent / Good / Fair / Poor) comes from the server's qualifier key. No client-side numeric thresholds exist. |
| Feedback headline | A short title chosen from the server's score feedback key. The keys are combinations of POSITIVE/NEGATIVE with descriptors such as long/short, deep, light, REM, restless, poor quality. | text | A second sentence says how the user may feel, chosen by the same key. |
| Personalized insight | A sentence chosen from the server's personalized-insight key. There are about 130 keys, in these groups: after an event day, event day, altitude, hard exercise, moderate exercise, jet lag, napping, pregnancy, late or irregular rhythm, stress. Performance (training-focused) devices get "training" wording; others get "activity" wording. If the key is unknown and looks like free text, the raw string is shown. | text | — |

**Score Factors** (expandable list under the header; see Lists)

| Factor | Value shown | Qualifier source |
|---|---|---|
| Duration | Total sleep time as h:mm | Duration qualifier key |
| Stress | "avg N", the average sleep stress. Hidden when ≤ 0 | Stress qualifier key |
| Deep | Deep minutes, rounded up (ceil(seconds/60)), shown as h m | Deep-percentage qualifier |
| Light | Light minutes, rounded up the same way | Light-percentage qualifier |
| REM | REM minutes, rounded up the same way | REM-percentage qualifier |
| Awake Time | Awake minutes, rounded up | Awake-count qualifier |
| Restlessness | Count of restless moments | Restlessness qualifier |
| Awake/Restless (sleep version 2 only; replaces the two rows above) | "awake duration • N restless moments" | Combined qualifier |

The qualifier word on each row maps Excellent, Good, Fair and Poor to their labels, and each word has its own colour role.

The optimal and ideal ranges for each factor (optimalStart/End, idealStart/EndInSeconds) are shown in the factor detail sheet (inferred).

**Total sleep time rule.** Total = light + deep + REM, where REM is counted only if the device is REM-capable. Awake time is never included. For manual or off-wrist nights, total = ceil(sleepTimeSeconds / 60) minutes.

**Sleep Metrics grid** (1 Day, two columns, in this order; each tile shown only when its capability and data are present)

| Tile | Source | Unit / format |
|---|---|---|
| Breathing Variations | Breathing disruption severity | Level word: 0 = Minimal, 1 = Few, 2 = Occasional, 3 = Frequent |
| Restless Moments | Restless-moments count (score-capable only) | integer |
| Avg Overnight HR | Average sleeping HR (requires the health-status capability) | bpm |
| Resting HR | Resting heart rate | bpm |
| Body Battery Change | Body Battery change over the night | Signed integer, "+N" or "−N" |
| Avg SpO2 / Lowest SpO2 | Average / lowest sleep SpO2 | % |
| Avg Respiration / Lowest Respiration | Average / lowest respiration | brpm |
| Avg Overnight HRV | Average overnight HRV | ms |
| 7d Avg HRV | HRV status word: Balanced, Unbalanced, Low, Poor, or No Status | text |
| Skin Temp | Average deviation from baseline in °C or °F, following the user's unit setting, signed. While calibration days remain, the tile reads "Calibrating" instead. | ±0.0° |

**7 Days / 4 Weeks, "Sleep Metric Averages"**

- **Header row:** average score (with its qualifier), average duration, average sleep need (coach only), average bedtime and average wake time. Times use the 12-hour or 24-hour clock per the user's setting.
- **Two-column tiles,** one per available overlay except HRV:
  - avg Resting HR (bpm)
  - avg overnight HR (bpm)
  - avg Body Battery change (rounded, signed)
  - avg SpO2 (%)
  - avg respiration (brpm)
  - avg skin temp change (°C/°F, signed)
- All of these come straight from the server's overall-stats block. The client only rounds them.

**1 Year:** each list row has Avg Score and Avg Duration. The chart tooltips have the weekly averages. Unknown whether a 1-year averages header exists; none was found in the decoded composable.

### Lists

- **Score Factors (1 Day).** The rows are in the order of the factor table above. The section is collapsed by default and expanded by tapping the arrow next to its title. Each row shows a title, a value and a qualifier word. Tapping a row opens a bottom sheet with an explanation of that factor and the user's value against the optimal range (inferred: the sheet content was only partly decoded).
- **Night list (7 Days / 4 Weeks).**
  - The list holds one row per night that has data, newest first (sorted descending by calendar date).
  - With score capability, each row shows the date, "Score" with its value, and "Duration".
  - Without score capability, each row shows the date and duration only.
  - Tapping a row jumps to the 1-day tab on that night.
- **Week list (1 Year).**
  - The list holds one row per week, sorted by week start date. The direction is inferred to be newest first.
  - With score capability, each row shows the week range, "Avg Score" and "Avg Duration".
  - Without score capability, each row shows the week range and the average duration.
  - Tapping a row jumps to the 7-day tab for that week.
- **Coach factor rows (1 Day, Coach page).** The rows are Sleep History, Activity History (or training-oriented wording on performance devices), HRV Status and Naps. Each shows a short status label, for example "Good", "Pattern of short sleep", "High load today", "One nap", or "More than two naps". Tapping a row opens an explanation sheet (inferred).

### Extra sections and empty states

- **Score page order (1 Day, normal device night):**
  1. Header (score, feedback, insight)
  2. Score Factors (expandable)
  3. Timeline/Stages switch with its chart
  4. Sleep Metrics grid
  5. Notes
- **Device night without score.** If the sleep was not recorded by a score-capable device, the page shows: stages, a "Sleep Timeline" section, the metrics grid and notes.
- **Manual or off-wrist night.** This applies when the sleep is not from a device and is confirmed off-wrist, or when it is manually confirmed with no automatic times. The page shows the duration, a short note that the sleep was entered manually, a hint to wear the device to bed for full data, and Notes.
- **No sleep for the date.** The page shows "No sleep data", a hint to wear the device to bed, an "Add Manual Sleep" button that opens the Add Sleep Times sheet, and a "Learn More" link to Help.
- **Coach page (1 Day).**
  - Tonight's sleep need, shown as hours and minutes, with a "Recommended" bedtime window from the server's recommended bedtime start and end minutes.
  - A one-line reason the need changed versus baseline: increased, highly increased, decreased, or no change. Past dates use past-tense wording.
  - The factor rows from Lists, including an "Add Nap" action under Naps.
  - When alignment is supported, a "Sleep Alignment" card. It shows a status line (aligned, ahead, behind, irregular, not aligned, not enough data yet, or still onboarding) plus a jet-lag variant with a "See Jet Lag Advisor" link when jet lag is auto-detected.
- **Insight/coach topic summaries** (one line each, own words):
  - Event day / after event day: race or event load affects tonight's need and last night's score.
  - Altitude: being at elevation can disturb sleep.
  - Hard or moderate exercise: recent training raises sleep need, or explains lighter sleep.
  - Jet lag: a time-zone shift has moved the internal rhythm.
  - Napping: naps reduce tonight's need, or cut into night sleep.
  - Pregnancy: pregnancy-specific sleep guidance.
  - Rhythm: late or irregular bedtimes reduce quality.
  - Stress: high stress kept the body from resting.
- **Naps.** The day's naps (dailyNapDTOS) feed the coach Naps factor. Naps can be added, edited or deleted through the nap endpoints. Unknown: the exact nap list layout on this screen. It lives in a separate naps package that was not decoded.
- **Notes.** There is a notes card on every day view. With no note, it shows a prompt ("Add Notes") asking how the user slept. With a note, it shows the note text and lets the user edit it. Saving an empty note deletes it; otherwise the note is stored. On success, the night reloads.
- **Edit sleep times.** Available from the overflow menu on 1 Day when the night allows editing. The user sets a start and end time and saves. A new night uses the create call; an existing night uses the update call. The night then reloads.
- **Cross-sell.** Below the 1-day content, an optional banner may promote an armband HR sensor (cross-sell).
- **Missing nights in multi-day views.**
  - The client fills date gaps with empty entries: one per day for 7d/4w, and one per week for 1y.
  - Charts show gaps. The score line is broken across them, and isolated nights become dots.
  - The night list omits nights without data (inferred).
  - A whole chart with no data shows its own empty text: no score data, no duration data, or no consistency data.
- **Loading and errors.** While loading, a spinner or placeholder is shown, driven by the shared report data-state renderer. Unknown: the error UI. It is likely the shared report retry state.

### Data needed

**Per night (1 Day)**

- **Endpoint:** a server endpoint.
- **Top-level fields:**
  -, `sleepLevels`, `sleepMovement`
  - `sleepRestlessMoments`, `restlessMomentsCount`
  - `breathingDisruptionData`
  - `wellnessEpochSPO2DataDTOList`, 
  - `wellnessEpochRespirationDataDTOList`, `wellnessEpochRespirationAveragesList`, `respirationVersion`
  - `sleepStress`, `sleepHeartRate`, `restingHeartRate`
  - `sleepBodyBattery`, `bodyBatteryChange`
  - `skinTempDataExist`, `skinTempData`, `avgSkinTempDeviationC/F`, `skinTempCalibrationDays`
  - `hrvData`, `hrvStatus`, `avgOvernightHrv`
- ** fields:**
  - Window: `calendarDate`, `sleepStart/EndTimestampGMT/Local`, `autoSleepStart/EndTimestampGMT`
  - Times: `sleepTimeSeconds`, `napTimeSeconds`, `deep/light/rem/awakeSleepSeconds`, `unmeasurableSleepSeconds`, `awakeCount`
  - Confirmation and source: `sleepWindowConfirmationType` (auto_confirmed, auto_confirmed_final, manually_confirmed, unconfirmed, off_wrist), `sleepFromDevice`, `deviceRemCapable`, `sleepVersion`
  - Score: `sleepScores`, `sleepScoreFeedback`, `sleepScoreInsight`, `sleepScorePersonalizedInsight`
  - Vitals: `avgSleepStress`, `averageSpO2Value`, `lowestSpO2Value`, `avgHeartRate`, `average/lowestRespirationValue`, `breathingDisruptionSeverity`
  - Other: `userNote`, `sleepNeed`, `nextSleepNeed`, `sleepAlignment`, `dailyNapDTOS`
- **`sleepScores`:** each of overall, totalDuration, stress, deepPercentage, lightPercentage, remPercentage, awakeCount and restlessness carries `value`, `qualifierKey`, `optimalStart/End` and `idealStart/EndInSeconds`.
- **Sleep need:** `baseline`, `actual`, `feedback`, `trainingFeedback`, `sleepHistoryAdjustment`, `hrvAdjustment`, `napAdjustment`, `recommendedBedtimeStart/EndMins`, `displayedForTheDay`, `deviceId`.
- **Sleep alignment:** `status`, `optimalSleepWindowStart/End/MidpointMins`, `lastSleepMidpointMins`, `sleepNeedBaselineMins`.

**Per window (7d/4w)**

- **Endpoint:** a server endpoint.
- **`individualStats[]`:** each entry has `calendarDate` and a `values` block with:
  - `sleepScore`, `totalSleepTimeInSeconds`, `sleepNeed` (minutes)
  - `deep/light/rem/awakeTime`
  - `local/gmtSleepStart/EndTimeInMillis`
  - `restingHeartRate`, `avgHeartRate`, `bodyBatteryChange`, `spO2`, `respiration`, `skinTempC/F`
  - `hrvStatus`, `avgOvernightHrv`, `hrv7dAverage`
  - `sleepAlignmentStatus`, `sleepAlignmentOswStart/End`
- **`overallStats`:**
  - `averageSleepScore`, `averageSleepSeconds`, `averageSleepNeed`
  - `averageLocalSleepStart/EndTime`
  - `averageRestingHeartRate`, `meanAvgHeartRate`, `averageBodyBatteryChange`, `averageSpO2`, `averageRespiration`, `averageSkinTempC/F`

**Per year (1y)**

- **Endpoint:** a server endpoint.
- **`individualStats[]`:** each entry has `weekStartDate`, `weekEndDate` and a `values` block with:
  - `averageSleepScore`, `averageSleepSeconds`, `averageSleepNeed`
  - `averageLocalSleepStart/EndTime`
  - `deep/light/rem/awakeTime`, `sleepDataDaysCount`, `sleepScoreQuality`
- **`overallStats`:** `averageSleepScore`, `averageHigh/LowSleepScore`, `averageSleepSeconds`, `averageSleepNeed`, `averageLocalSleepStart/EndTime`.

**Writes**

| Purpose | Request |
|---|---|
| Add a manual night | a server endpoint |
| Edit a night's times | a server endpoint |
| Save a note | a server endpoint; the body holds the note text |
| Delete a note | a server endpoint |
| Add a nap | a server endpoint |
| Edit a nap | a server endpoint |
| Delete a nap | a server endpoint |
| Read naps | a server endpoint |

**Also loaded by the ViewModel** (for coach factors): the HRV status, intensity minutes and training-status/acute-load reports for the selected date. These are shared with the other report screens.

**Capability gates**

These are device-capability flags from the user's device list. Each one hides the matching section, tile or overlay chip when it is absent:

- sleep score
- sleep need/coach
- sleep alignment
- skin temp
- REM
- breathing variations
- all-day HR
- health status (overnight HR)
- Body Battery
- pulse ox
- respiration
- HRV status

A "performance device" flag switches insight wording to training.

## HRV Status

### Navigation
- Entry points: the My Day HRV card, the calendar/day views, and a deep link.
  - Tapping the card body opens the page on today.
  - Tapping the top area of the card opens it without a date or tab, so the screen defaults
    apply: today, first tab (1d) (inferred; the base activity class is missing).
  - Tapping the bottom area opens it on today with the 4w tab selected.
- Title: "HRV Status". A help action sits in the top bar and opens the help page (see Extra
  sections).
- Tabs: "1d", "7d", "4w", in that order. The default is 1d (inferred, see above).
- Paging: each tab is a horizontal pager with one page per period.
  - 1d: one page per calendar day.
  - 7d / 4w: the last page is the window ending today. Each earlier page steps back by 7 or 28
    days (non-overlapping rolling windows).
  - The earliest reachable date is 2006-01-01. Paging into the future is not possible.
- Period line under the tabs:
  - 1d: a single date.
  - 7d / 4w: "start – end" date range.
- Optional banner at the top: a sleep-education banner about missing sleep data, with a
  learn-more link. It is shown or hidden by a separate sleep-education view model. The conditions
  are Unknown, because the decision lives in a view model whose rules were not traced.

### Charts
**1d tab**

1. Status gauge (horseshoe / three-quarter ring)
   - Four arc segments, from left to right:

     | Segment | Arc share | Colour role |
     |---|---|---|
     | 1 | 12.5 % | "low" |
     | 2 | 12.5 % | "unbalanced" |
     | 3 | 50 % | "balanced" |
     | 4 | 25 % | "unbalanced" |

   - Status POOR: only segment 1 is coloured, in the "poor" role. The rest is drawn empty.
   - Status NONE or INVALID: all segments are empty (neutral).
   - Marker:
     - Position = baseline.markerValue (0..1, rounded to a percent).
     - The marker is placed in the segment that matches the status: LOW → 1, UNBALANCED_LOW → 2,
       BALANCED → 3, UNBALANCED_UPPER → 4.
     - A plain UNBALANCED status is resolved to upper or lower: upper when weeklyAvg is at or
       above balancedUpper, otherwise lower.
   - Centre text: weeklyAvg + "ms", with the small label "7d Avg". Shows "--" when the value is
     not positive.
   - The gauge is drawn whenever a status exists, or when at least a weeklyAvg exists. In that
     second case the baseline is empty and only the centre value shows.
   - Under the gauge:
     - Status word ("Balanced", "Unbalanced", "Low", "Poor", "No Status"), or a no-data line.
     - A one-sentence feedback text chosen by feedbackPhrase (see Summary numbers).
     - For the onboarding feedback phrases, a "learn more" link about consistent sleep tracking
       is appended.

2. "Nightly Timeline": line chart of the 5-minute HRV readings across the night.
   - x axis:
     - Runs from sleep start to sleep end. Falls back to the start/end timestamps when sleep
       times are missing.
     - Labels: start time, a tick at each full hour, and end time.
     - Small icons mark the ends: a sleep icon at the start (inferred) and a wake/alarm icon at
       the end.
   - Data cleaning before plotting:
     - Drop readings before sleep start.
     - Pad the leading gap from sleep start to the first reading.
     - Where two readings are more than 5 minutes apart, insert empty slots.
     - Cut the list at sleep end.
     - Only values > 0 are drawn, so empty slots appear as gaps in the line.
   - y axis:
     - Range from (min − 10) to (max + 10), rounded to a tidy step.
     - With no data, the ticks default to 50 / 70 / 90 / 110.
   - Line colour: primary "HRV" role.
   - Scrubbing: touch-and-drag shows a tooltip with:
     - the 5-minute window "HH:MM–HH:MM" that ends at the touched reading;
     - the value in ms with label "HRV".

     An empty slot shows the time window only.
   - Empty state: if lastNightAvg is missing, or there are no readings, the chart is replaced by
     a "No Nightly HRV Data" title and a short explanation. If an average exists without
     readings, the chart is simply hidden.

**7d and 4w tabs (same layout, different length)**
- Chart card "7-day Averages": a combined chart with one x slot per day of the window.
  - Series 1 (main): a scatter point per day.
    - Value = that day's weeklyAvg.
    - Each point is drawn as a small status icon, coloured by the status role.
  - Series 2: the baseline band.
    - Shaded area between balancedLow and balancedUpper, drawn in a muted "baseline" role.
    - Omitted on days with status POOR or NONE, or when the baseline values are not positive.
    - The band breaks at such days.
  - Series 3 (optional overlay): a dashed line of lastNightAvg per day.
    - Breaks at missing days.
    - Toggled by the chip "Overnight Averages". The choice is remembered across sessions.
  - Legend: shows only the items that are actually present, in this order:
    1. Balanced
    2. Unbalanced
    3. Low
    4. Poor
    5. No Status
    6. Baseline
    7. Overnight Avg
  - x axis: labels only on the first and last day, as short dates. The x range is padded by
    about 0.07 of a slot on each side.
  - y axis:
    - Computed from the min and max over weeklyAvg, lastNightAvg, balancedLow, balancedUpper and
      lowUpper across the window.
    - Rounded outward to a "nice" scale with 4 intervals. The step is a multiple of 1 or 5,
      chosen by trying growing padding factors (1.1 up to 2.0) until the scale fits.
    - Empty window: 0..110.
  - Tooltip on tap or scrub of a day:
    - The date as a long month name plus day.
    - Then the touched item:
      - 7d Avg: value + "ms", with its status icon.
      - Baseline: the "low–upper" range in ms.
      - Overnight Avg: value + "ms".
  - Missing days: placeholder entries keep the x slots, but draw nothing.
- No-data: the chart renders empty and a "No HRV Status data" style message is shown.

### Summary numbers
**1d tab**
- 7d Avg (gauge centre) = weeklyAvg, integer ms.
- Status = enum `status`, mapped to a word and a colour role:

  | status | Word | Colour role |
  |---|---|---|
  | BALANCED | Balanced | balanced |
  | UNBALANCED, UNBALANCED_LOW, UNBALANCED_UPPER | Unbalanced | unbalanced (shared) |
  | LOW, POOR | Low / Poor | shared low-poor role |
  | NONE, INVALID | No Status | neutral grey |

- Feedback sentence:
  - Picked from feedbackPhrase (families: onboarding, poor, low, unbalanced, balanced, each with
    several numbered variants).
  - The builder writes its own short sentence per family.
  - Unknown variants fall back to no sentence.
- Stats row:
  - "Overnight Avg" = lastNightAvg in ms.
  - "Highest 5-min Avg" = lastNight5MinHigh in ms, or "--".

**7d / 4w tabs**
- Footer with two numbers, both in ms and rounded down (floor):
  - Mean of lastNightAvg over the days that have a value.
  - Mean of weeklyAvg over the days that have a value.
- "--" when there is nothing to average.
- Note: in this build the two footer numbers are labelled with generic "Sleep Avg" / "Awake Avg"
  resource labels, which look reused from another screen. Suggested labels for Nimrag:
  "Overnight Avg" and "7d Avg".

### Lists
**7d / 4w tabs only**: a table below the chart.
- One row per day returned by the API. The API order is used, which is probably ascending by
  date (inferred).
- Row content:
  - Weekday name and short date.
  - The day's 7d Avg ("N ms" or "--"), with a small status icon.
  - The day's Overnight Avg ("N ms" or "--").
- Tapping a row opens that date (inferred: switches the screen to the 1d tab on that date,
  through the summary navigator).

The 1d tab has no list.

### Extra sections and empty states
- Help page topics (one line each):
  - What HRV is and why overnight readings are used.
  - How a status is earned: wear the watch during sleep for about three weeks to build a
    baseline.
  - How to read the status, the baseline range and the 7-day average.
  - A link to an external research article.
- Empty states:
  - 1d with no data: "No data" status line, empty gauge, and the nightly-chart empty message.
  - Range with no data: empty chart and an empty table.
  - A day without data inside a range keeps its x slot and shows "--" in the table.
- Onboarding: during the first weeks the status is NONE and the feedback phrase is an onboarding
  variant. Show the gauge empty, plus the learn-more link.

### Data needed
- a server endpoint → day object:
  - `userProfilePk`
  - `hrvSummary` (see the summary fields below)
  - `hrvReadings[]`: `hrvValue` (int ms), `readingTimeGMT`, `readingTimeLocal`
  - `startTimestampGMT/Local`, `endTimestampGMT/Local`
  - `sleepStartTimestampGMT/Local`, `sleepEndTimestampGMT/Local`
- a server endpoint → `{ hrvSummaries: [summary] }`.
- Summary fields:
  - `calendarDate`
  - `weeklyAvg`, `lastNightAvg`, `lastNight5MinHigh`
  - `baseline { lowUpper, balancedLow, balancedUpper, markerValue (0..1) }`
  - `status`, `feedbackPhrase`, `createTimeStamp`
- Local preference: the overlay toggle (Overnight Averages on/off).

## Respiration

### Navigation
- Opened from the My Day respiration card. Two optional launch extras: initial date and initial
  tab. When absent, the page opens on today and the 1d tab.
- Title: "Respiration". A help action sits in the top bar (see Extra sections).
- Tabs: "1d", "7d", "4w". Default is 1d.
- Header under the tabs: previous/next arrows around a period text.
  - 1d: the date.
  - 7d / 4w: the "start – end" range of a rolling window ending on the anchor date.
  - Arrows (and horizontal swipe) move one period back or forward. Moving past today is not
    possible.
- Data versions:
  - The day payload carries `respirationVersion`. Version 200 means newer devices with hourly
    averages.
  - Version 200 switches the 1d chart type, the help topics and the sleep-report variant.

### Charts
**1d tab, version 200: "Daily Timeline"**
- x axis: minutes from local midnight to the next midnight, slightly padded at both ends.
- Main series:
  - One point per hour, placed at that hour's minute offset. The value is the hourly average,
    rounded.
  - Consecutive valid hours are joined by a line. Hours without a value break the line.
  - Colour: primary "respiration" role.
- Special hourly codes inside the averages array:
  - avg = −1: the hour was too active to measure. It is drawn as a full-height bar in an
    "active" role, only when the "Active" overlay is on.
  - avg = −2: unmeasurable. It leaves a gap, and adds the legend item "Unmeasurable".
- Chips (toggles; the selection is remembered):
  - "High/Low Rates": shown only when high and low data exist. Draws, per hour, a thick vertical
    bar from the low to the high value, with an end dot in a "low" role and one in a "high"
    role.
  - "Active": shown only when at least one hour is flagged too active.
- y axis:
  - Default 8..24 brpm.
  - With high/low data: min = max(0, dataMin − 1) and max = dataMax + 1, then widened so the
    span divides evenly by 4.
  - Five horizontal grid lines, labelled at min, the three quarter points and max.
- Background: sleep periods from the sleep timeline are shaded in a "sleep" role.
- Tooltip on scrub:
  - The hour range.
  - Average value with the label "Avg Respiration Rate" and unit "brpm".
  - With the overlay on, the "Low" and "High" values as well.

**1d tab, older versions (raw chart)**
- One line of the raw readings (about one every 2 minutes) from start of day to end of day.
- x axis: labels every 4 hours, aligned to the data, with hourly ticks.
- y axis: (min − 1)..(max + 1); default 8..24.
- Icons mark sleep start and wake time for the night, and for the next night when present.
- Empty: "No Respiration Data".

**Both 1d variants**
- The chart sits inside a "processing" wrapper. If raw values exist but the lowest, highest or
  awake average is still not positive, a "still processing" message shows with a reload action.
  Reloads are capped per day. When the cap is reached, a "cannot reload" message shows.

**7d and 4w tabs**
- Line chart with dots; the title "Daily Averages" appears in version 200 only.
- Two series, one point per day at the day index:
  - Sleep average (avgSleepRespiration), in a "sleep" role.
  - Awake average (avgWakingRespiration), in an "awake" role.
- Missing days keep their slot (the data is pre-filled with one entry per day) and draw nothing.
- y axis: min = min(8, dataMin), max = max(20, dataMax).
- x axis: one slot per day, with date labels spread across the window.
- Legend: "Sleep Avg", "Awake Avg".
- Tooltip: the date (year, short month, day), plus both values in brpm.
- Empty: "No Respiration Data".

### Summary numbers
**1d tab**: three stat tiles under the chart. Values are integers truncated toward zero, in brpm,
with "--" when the value is not positive.

| Tile | Field |
|---|---|
| "Lowest" | lowestRespirationValue |
| "Highest" | highestRespirationValue |
| "Awake Avg" | avgWakingRespirationValue |

**1d tab, "Sleep Averages" section**
- Shown when the date is valid and at least one of avgSleepRespirationValue or
  avgTomorrowSleepRespirationValue is above 0.
- Both present: two tiles side by side.
  - Weekday of the page date, with avgSleep.
  - Weekday of the next date, with avgTomorrowSleep.
- Only one present: a single row with weekday, full date and value.
- Tapping a tile opens the Sleep detail page for that date. The version-200 sleep variant is
  used when applicable.

**7d / 4w tabs**: Unknown. The range content composable that wraps the chart is missing from the
decompile, and the visible chart builder renders only the chart.
- Recommended (inferred, matching the sibling Pulse Ox and HRV pages):
  - Window means of sleep and awake average.
  - A per-day list, newest first.

### Lists
- 1d: no list besides the Sleep Averages tiles.
- 7d / 4w: Unknown (see above). Inferred recommendation:
  - One row per day: weekday, date, sleep avg and awake avg in brpm, "--" for missing.
  - Tapping a row opens that day on the 1d tab.

### Extra sections and empty states
- Help topics (one line each):
  - What respiration rate is and how the watch measures it.
  - What is needed to receive data: a compatible device, worn through the day and night.
  - How to read the numbers (awake versus sleep, normal variation).
  - How the hourly averages chart works, including active and unmeasurable hours (version 200
    only).
- Empty states:
  - 1d without data: chart empty text, tiles show "--", and the Sleep Averages section is
    hidden.
  - Range without data: empty chart text.
  - The processing/reload state is described under Charts.

### Data needed
- a server endpoint → day object.
  - Identity and window:
    - `userProfilePK`, `calendarDate`
    - `startTimestampGMT/Local`, `endTimestampGMT/Local`
  - Sleep times:
    - `sleepStartTimestampGMT/Local`, `sleepEndTimestampGMT/Local`
    - `tomorrowSleepStartTimestampGMT/Local`, `tomorrowSleepEndTimestampGMT/Local`
  - Averages and extremes:
    - `avgWakingRespirationValue`, `avgSleepRespirationValue`,
      `avgTomorrowSleepRespirationValue`
    - `lowestRespirationValue`, `highestRespirationValue`
  - `respirationValuesArray`: rows of [timestampMs, value].
  - `respirationAveragesValuesArray`: rows of [timestampMs, avg, high, low]. avg −1 means too
    active; avg −2 means unmeasurable.
  - `respirationVersion` (200 = hourly averages).
- The sleep timeline for the same date is used for the shaded sleep periods. It comes from the
  sleep service's timeline/snapshot payload, the same one the Sleep page uses.
- a server endpoint → array of
  `{ calendarDate, avgWakingRespiration, avgSleepRespiration }`. The client fills in missing
  days.
- Local preference: the selected overlays (High/Low, Active).

## Pulse Ox

### Navigation
- Opened from the My Day Pulse Ox card on today.
  - A second card action calls a separate helper with the current time. Its purpose is Unknown,
    because the helper was not traced.
  - The acclimation card is a different screen and is out of scope here.
- Launch extras: initial date and initial tab. The defaults are today and 1d.
- A consent banner can appear at the top. It is about health-data consent, and its rules were
  not traced (Unknown).
- Title: "Pulse Ox", with back and help actions. In Japan the help offers an extra action about
  blood oxygen.
- Tabs: "1d", "7d", "4w". Default is 1d.
- Header: previous/next arrows around the period text.
  - 1d: the date.
  - 7d / 4w: the "start – end" range of a rolling window ending on the anchor date.
  - Swiping or the arrows step one period. Moving past today is not possible.

### Charts
**1d tab**

1. Gauge card (top)
   - Shown value:
     - Today: the latest reading.
     - Other days: the day's average, falling back to the latest reading.
   - Text: "N%", or "--".
   - Gauge:
     - Arc from 59 % to 100 %, split into 4 equal segments coloured from a "low" role to a
       "normal" role.
     - Marker at (clamp(value, 59, 100) − 59) / 41.
     - All segments neutral when there is no value.
   - Centre label: "Avg SpO₂" when an average is shown, "SpO₂" when the latest reading is shown.
   - Today only: a "Last Read" line with the time of the latest reading.

2. Day chart
   - Wrapped in the same "processing / reload" wrapper as Respiration. The incomplete state
     applies when there is no positive average and no hourly or single values (the exact
     condition is partly garbled).
   - x axis:
     - Minutes of the day window.
     - Labels every 4 hours, with hourly ticks.
     - The sleep window is shaded in a "sleep" role.
   - y axis: 60..102 %, with 5 labels.
   - Hourly averages, one 60-minute-wide bar per hour:
     - The bar is transparent from 0 up to the value.
     - It is filled from the value up to 100 in the colour of the value's band.
     - The visual effect: a coloured cap that grows downward as saturation drops.
   - Single (spot) readings: small dots at their time, coloured by band.
   - Bands, each with its own colour role:

     | Band | Range |
     |---|---|
     | harmful | below 70 |
     | low | 70–79.999 |
     | moderate | 80–89.999 |
     | normal | 90–100 |

   - Legend:
     - The four band labels: "<70%", "70-79%", "80-89%", "90-100%".
     - A sleep-shading item when the sleep window overlaps the day.
   - Touch: enabled only when data exists. The tooltip shows the time or hour and the value in %
     (detail inferred).
   - Empty: "No data available".

**7d and 4w tabs**
- Card titled "Pulse Ox": a stacked bar chart, about 170 dp tall, using the same cap technique
  and band colours as the 1d bars.
- 7d window (7 days or fewer):
  - One bar per HOUR across the whole week, from the hourly-average series (24 × 7 slots).
  - x labels: weekday name at noon of each day.
  - Data is shown only if at least one hourly value is above 0.
- 4w window (more than 7 days):
  - One bar per DAY, from the daily-average series.
  - x labels: one numeric short date per 7-day block, placed mid-block, showing the block's
    first day.
- y axis: 60..100 %, with 5 labels.
- x range: from −0.5 to slotCount − 0.5.
- Legend: the four band labels.
- Empty: "No data".

### Summary numbers
**1d tab**
- Gauge value as above, as an integer percent.
- Today only: a stat tile "Avg SpO₂", showing the average with a fallback to the latest reading,
  in %.
- Values are truncated to integers. A missing value shows "--" followed by "%".

**7d / 4w tabs**
- One stat tile with the window's overall average SpO₂ (`overallSpo2Average`, truncated to an
  integer, "%").
- Shows "--" when the value is not positive.

### Lists
**1d tab, "Single Readings"**
- Shown only when spot readings exist.
- One row per reading, in API order (probably chronological, inferred).
- Row content:
  - Time of day, in 12- or 24-hour format following the device setting.
  - Value "N %".
  - A colour marker in the reading's band colour.
- Rows are not tappable (they are only grouped for accessibility).

**1d tab, sleep section**
- Same pattern as Respiration.
- Shown when avgSleepSpO2 or avgTomorrowSleepSpO2 is above 0.
- Up to two rows/tiles:
  - The page date with avgSleepSpO2.
  - The next date with avgTomorrowSleepSpO2.
- Each shows the weekday, the date (when only one is present), the value in % and a small band
  gauge.
- Tapping opens that date's sleep page (inferred).

**7d / 4w tabs, day list**
- One row per day of the window, newest first: the loop runs from the end date back to the
  start date.
- Row content:
  - Weekday and date.
  - Daily average "N %", or "--" when there is no data.
  - A small band-coloured mini gauge.
- Tapping a row calls back with that date (inferred: opens the 1d tab on that date; the wiring
  class is missing).

### Extra sections and empty states
- Help topics (one line each), with separate wording for Japan:
  - What a pulse oximeter reading represents.
  - How and when the watch takes readings, and why staying still matters.
  - What affects accuracy: fit, movement, skin and circulation.
  - Pulse Ox during sleep and how to enable it.
  - A link to more information.
- Empty states:
  - 1d without data: gauge shows "--" with neutral segments, the chart shows the empty text,
    and the readings and sleep sections are hidden.
  - Range without data: chart empty text, average "--", and every list row shows "--".

### Data needed
- a server endpoint → day object.
  - Identity and window:
    - `userProfilePK`, `calendarDate`
    - `startTimestampGMT/Local`, `endTimestampGMT/Local`
  - Sleep times:
    - `sleepStartTimestampGMT/Local`, `sleepEndTimestampGMT/Local`
    - `tomorrowSleepStartTimestampGMT/Local`, `tomorrowSleepEndTimestampGMT/Local`
  - Values:
    - `averageSpO2`, `lowestSpO2`, `lastSevenDaysAvgSpO2`
    - `latestSpO2`, `latestSpO2TimestampGMT/Local`
    - `avgSleepSpO2`, `avgTomorrowSleepSpO2`
  - `spO2SingleValues`: spot readings (timestamp, value).
  - `spO2HourlyAverages`: hourly averages (timestamp, value).
  - `monitoringEnvironmentValues`: altitude-related data. It is not used by this page's main
    widgets.
- a server endpoint →
  range object.
  - `overallSpo2Average`.
  - `spo2HourlyAverageArray`: rows of [timestampMs, value]. Its columns are described by
    `spo2AccValueDescriptorList`.
  - `spo2DailyAverageArray`: rows of ["yyyy-MM-dd", value]. Its columns are described by
    `spo2DailyAcclimationAverageDescriptorList`.
  - `monitoringEnvironmentValuesArray` and `monitoringEnvironmentValueDescriptorList`: not
    needed for this page.

## Training Readiness

### Navigation
- Entry:
  - the Training Readiness card on My Day;
  - morning report and notifications, which can pass a start date.
- Title: "Training Readiness".
- No tabs. The screen shows one day at a time:
  - previous/next arrows in a date header, plus swipe paging between days;
  - opens on today, or on the date passed in;
  - next is limited to today (inferred).
- The date header shows "Today", "Yesterday" or the date.
- Top of page:
  - an optional promo slot;
  - a sync/device banner when the readiness device needs syncing or updating.
- Menu: "Help".
- The page root did not decompile. The single-day layout is inferred from the date-pager and header wiring.

### Charts
- **Score gauge** (top of the page): a ring/arc gauge showing 0–100.
  - Coloured segments by level:

    | Level | Score range |
    |---|---|
    | Poor | 0–25 |
    | Low | 25–50 |
    | Moderate | 50–75 |
    | High | 75–95 |
    | Prime | 95–100 |

  - Each level uses its own colour role, a scale from bad to good.
  - It is not a time chart and has no scrubbing.
- **Factor sheets** (bottom sheet opened by tapping a factor):
  - Acute load: shows the value and qualifier, a feedback sentence, and a small daily acute-load chart against the optimal-range band (same style as the Training Load tab). The window length is Unknown.
  - HRV: shows the HRV status sheet. Stress history: shows a short stress-history chart. Both are reused from those features.
  - Sleep, sleep history and recovery time: content is Unknown, because is missing.

### Summary numbers
- **Updated line:** "Updated <time> · <context>".
  - The time comes from `timestampLocal`.
  - The context comes from `inputContext`:
    - AFTER_WAKEUP_RESET → "After wake-up";
    - AFTER_POST_EXERCISE_RESET → "After activity";
    - UPDATE_REALTIME_VARIABLES → "After change", i.e. a level change during the day.
- **Score:** integer `score` 0–100, with "N of 100" under it.
  - "--" is shown when `validSleep` is not true or a recent dive blocks the score.
- **Level:** from `level` (NONE, POOR, LOW, MODERATE, HIGH, PRIME, INVALID, RECENT_DIVE).
  - After a recent dive, the level shows "Recent Dives" with a note that the score is unavailable.
- **Short feedback and long feedback:**
  - Chosen from enumerated keys `feedbackShort` and `feedbackLong`. Write the text fresh.
  - If sleep was not valid, an extra note asks the user to wear the watch overnight.
- **Which record is shown for a day:**
  - The API can return several records per day.
  - Today: the first record, which is the latest.
  - Past days: sort newest first by `timestampLocal`, then take the first match from this list:
    1. a wake-up record from the primary training device;
    2. any wake-up record;
    3. the newest record with a known context.
  - For past days, the acute-load factor fields are replaced with those from the latest record of that day.
- **Recent dive:** behind a feature flag, the app loads activities from yesterday until now.
  - If a dive-type activity ended within the last 24 h, `minutesSinceLastDive` is set and the score is suppressed.

### Lists
- **Factors**, in this fixed order, one row each: label, value, qualifier and a chevron.
  1. "Recent Dives":
     - Only when a dive happened in the last 24 h.
     - Value = hours left until 24 h have passed.
     - The advice depends on minutes since the dive:
       - ≤ 90: no exercise;
       - ≤ 240: no strenuous exercise;
       - otherwise: no intense exercise.
  2. "Sleep Score": value `sleepScore`; qualifier Poor / Fair / Good / Excellent from `sleepScoreFactorFeedback`.
  3. "Recovery Time": value in h or d (see Recovery Time); qualifier High / Moderate / Low need or Fully recovered.
  4. "HRV Status": value `hrvWeeklyAverage` ms; qualifier Poor / Low / Unbalanced / Balanced from `hrvFactorFeedback`.
  5. "Acute Load": value `acuteLoad` as an integer; qualifier Very high / High / Optimal / Low from `acwrFactorFeedback`.
  6. "Sleep History": no value; qualifier Poor / Fair / Good / Excellent from `sleepHistoryFactorFeedback`.
  7. "Stress History": no value; qualifier High / Medium / Low from `stressHistoryFactorFeedback`.
- A factor with feedback "NONE" or missing:
  - shows "no data on device", or "not enough data" for HRV;
  - is drawn disabled and cannot be opened.
- The `*FactorPercent` values are NOT shown on this page. The My Day card uses them in a two-column factor grid.

### Extra sections and empty states
- Help topics (one line each):
  - what readiness is;
  - how each factor contributes;
  - when the score updates (wake-up, after activity, during the day);
  - why sleep must be recorded;
  - the dive rule.
- Empty state (no record for the day): a message saying the device must be worn continuously for about three days, including sleep, before a score appears. The factor list is hidden.
- Missing days: the pager still lands on the day and shows the empty state.

### Data needed
- a server endpoint returns a list of records for the day. Fields:
  - `calendarDate`, `timestamp`, `timestampLocal`, `deviceId`, `userProfilePK`, `primaryActivityTracker`, `inputContext`;
  - `score`, `level`, `feedbackShort`, `feedbackLong`, `validSleep`;
  - `sleepScore`, `sleepScoreFactorPercent`, `sleepScoreFactorFeedback`;
  - `recoveryTime`, `recoveryTimeFactorPercent`, `recoveryTimeFactorFeedback`;
  - `hrvWeeklyAverage`, `hrvFactorPercent`, `hrvFactorFeedback`;
  - `acuteLoad`, `acwrFactorPercent`, `acwrFactorFeedback`;
  - `sleepHistoryFactorPercent`, `sleepHistoryFactorFeedback`;
  - `stressHistoryFactorPercent`, `stressHistoryFactorFeedback`.
- The range form a server endpoint is used by the My Day card.
- Factor sheets:
  - HRV: a server endpoint and the HRV range report.
  - Acute load: the training-status weekly report (`ACUTE_LOAD_DAILY_TRAINING`, tunnel min/max).
  - Sleep: a server endpoint, a server endpoint.
  - Stress: a server endpoint.
- Dive rule: a server endpoint (yesterday → now), filtered to dive types.

## Training Status

### Navigation
- Entry: the Training Status card on My Day. It also opens from Training Readiness and the performance stats list.
- Screen title: "Training Status".
- Four tabs in a fixed row: "Current", "4w", "12w", "6m".
  - The default tab is Current.
  - A caller can ask for the summary view, which opens on 4w instead.
- Current tab: a single page that shows the latest state. There is no date paging.
- History tabs (4w, 12w, 6m):
  - Each tab is an endless horizontal pager of date windows.
  - The newest window ends today and runs back 28, 84 or 182 days (4w / 12w / 6m). The end of the day counts as included.
  - Swiping back moves the window back by its own length; there is no gap and no overlap.
  - The period line under the tabs shows the window as a date range, e.g. "Oct 24 – Nov 20".
- After the first swipe the app shows a one-time hint about syncing (inferred: a hint that history depends on synced devices).
- Overflow menu:
  - "Help" opens the help topics for training status.
  - "Pause" or "Resume" asks for confirmation, then changes a user setting. While paused, the status shows as paused from the pause date onward.
  - "Device priority" appears only when more than one device records training status. It opens the device-priority settings.
- Banner: if the primary training device last synced 7 or more days ago, a top banner asks the user to sync or update the device.
- When an adaptive training plan is active, pausing first shows a prompt explaining that the plan will also be affected (inferred from the plan-pause dialog wiring).

### Charts
- **Current tab:** no chart. The page is made of a status block plus a list of link rows (see Lists).
- **4w / 12w / 6m tabs:** a "caterpillar" chart.
  - Type: one horizontal bar across the full width, split into coloured segments.
  - One segment is one unbroken run of days that share the same status.
  - Segment width = (days in run + 1) / days in window × 100 % of the bar.
  - Segment colour = the status colour role (see Summary numbers). Paused runs use a separate neutral "paused" role and are flagged as paused.
  - Runs with an unknown status are kept for width but drawn invisible, so they leave a gap.
  - X-axis labels under the bar:
    - 4w and 12w: week labels, one per 7 days, from the shared date-label helper.
    - 6m: six short month names. Label i sits at day (2i + 1) / 12 × window length + 1, i.e. at the middle of each sixth of the window.
    - Blank labels are dropped.
  - Tapping a segment highlights it and the matching row in the table below. Tapping the same segment again clears the highlight.
  - There is no scrubbing and no y-axis.
- Data source per tab:
  - 4w and 12w: the weekly training-status report for the window. The 4w case is missing from the decompiled branch, so 4w = weekly is inferred.
  - 6m: the monthly training-status report.
  - Results are cached per (start, end) pair.

### Summary numbers
- **Status** (Current tab): one value from the device's daily record, chosen by the `trainingStatus` number:

  | Code | Status |
  |---|---|
  | 0 | No status (unknown) |
  | 1 | Detraining |
  | 2 | Unproductive |
  | 3 | Overreaching |
  | 4 | Maintaining |
  | 5 | Recovery |
  | 6 | Peaking |
  | 7 | Productive |
  | 8 | Strained |
  | missing / NaN | No data |
  | `trainingPaused` true | Paused (overrides the code) |

  Each status has an icon, a colour role, a label and a short help text.
- **Since date:** "Since <MMM d>", from `sinceDate`. For a paused status it shows the pause date instead.
- **Feedback message:** a sentence chosen by the `trainingStatusFeedbackPhrase` key. If there is no key or no data, a generic "no status yet" message is shown.
- **Load focus headline:** "Load Focus | <headline>". It is shown only when the record carries the acute/chronic load block (newer devices). The headline comes from the load-balance feedback key (see Training Load, Load Focus tab).
- **VO2 source line:** names the sport the status is based on: running, cycling or rowing. It uses `fitnessTrendSport`, else `sport`. It is hidden when the status is unknown or the device is in a mode that does not show VO2.
- **Provisional status:** when the status is provisional, a line asks the user to sync the named device.
- **Paused state:** a "Resume" button is shown on the page.
- **Unknown state:** a "How to get status" help button replaces the details.

### Lists
- **Current tab: link rows**, each opening another screen:
  1. "VO2 Max": the integer value plus its category (Poor … Superior). Opens VO2 Max on the matching sport.
  2. Load row:
     - Label "Load" when the acute/chronic block exists, otherwise "7-Day Load".
     - Value: the integer load plus a qualifier (Low / Optimal / High / Very high).
     - Opens Training Load, on the Load Focus tab when advanced load is on.
  3. "HRV Status": weekly average in ms plus status word. Opens HRV Status.
  4. "Altitude Acclimation":
     - Value in metres. Shown only if > 0 and the device supports it.
     - Tapping opens a small dialog with the date (Today / Yesterday / date), current and previous acclimation altitude, and a trend.
  5. "Heat Acclimation": a percentage, shown only if > 0. Opens the heat/altitude acclimation screen.
- **Device selector:** shown when two or more devices recorded status. Choosing a device switches all values to that device's record. The last choice is stored as a user preference.
- **History tabs: a two-column table** under the caterpillar chart.
  - Columns: "Status" and "Dates".
  - One row per visible segment, newest first.
  - Dates column:
    - "Since <date>" for the run that is still ongoing;
    - "start – end" for a closed run;
    - a single date for a one-day run.

### Extra sections and empty states
- Help topics (one line each, own words):
  - what each status means;
  - how load, VO2 trend and HRV combine into status;
  - how to receive a status (needs VO2 activities plus continuous wear);
  - why a second device can disagree (device priority);
  - what pausing does.
- History tabs show two info notes under the table:
  - a general explanation;
  - a hint for when no status was recorded.
- Empty states:
  - Current tab with no record: the no-data message plus the "How to get status" button. Link rows with no value are hidden.
  - History tab with no segments: the chart and table are hidden, and only the no-data message shows.
- Missing days: days without a record end a run. Inside a window they appear as an invisible (unknown) gap in the bar and do not appear in the table.

### Data needed
- a server endpoint returns the current page data:
  - `mostRecentTrainingStatus.payload`:
    - `recordedDevices[]` (deviceId, name, image), `showSelector`, `lastPrimarySyncDate`;
    - `latestTrainingStatusData{<deviceId>: record}`.
  - Each record:
    - `calendarDate`, `sinceDate`, `trainingStatus`, `trainingPaused`, `trainingStatusFeedbackPhrase`;
    - `fitnessTrend`, `fitnessTrendSport`, `sport`, `subSport`;
    - `weeklyTrainingLoad`, `loadTunnelMin`, `loadTunnelMax`, `loadLevelTrend`;
    - `primaryTrainingDevice`, `deviceId`, `timestamp`;
    - `acuteTrainingLoadDTO{dailyTrainingLoadAcute, dailyTrainingLoadChronic, minTrainingLoadChronic, maxTrainingLoadChronic, dailyAcuteChronicWorkloadRatio, acwrPercent, acwrStatus, acwrStatusFeedback}`.
  - `mostRecentVO2Max.payload{generic, cycling, rowing, heatAltitudeAcclimation}`: VO2 value and category for the link row.
  - `mostRecentTrainingLoadBalance.payload{recordedDevices, metricsTrainingLoadBalanceDTOMap{<deviceId>: balance}}`: used for the load-focus headline.
  - `mostRecentHeatAltitudeAcclimation.payload`: altitude and heat values.
- a server endpoint (4w, 12w) and `.../monthly/{start}/{end}` (6m) return:
  - `reportData`, `weeklyTrainingStatus`, `monthlyTrainingStatus`, `recordedDevices`, `metricsMap`, `allMetrics`;
  - per-day metric series keyed `TRAINING_STATUS`, `TRAINING_STATUS_PAUSED`, `ACUTE_LOAD_DAILY_TRAINING`, `CHRONIC_LOAD_DAILY_TRAINING`, `MIN/MAX_TRAINING_LOAD_CHRONIC`, `DAILY_ACUTE_CHRONIC_WORKLOAD_RATIO`, `ACUTE_LOAD_STATUS`, `ACUTE_LOAD_FEEDBACK`, `ACUTE_LOAD_PERCENTAGE`.
- a server endpoint: data for the altitude dialog.
- HRV weekly average comes from the HRV service (see the HRV section).
- User preference `TrainingStatus.last.device` (userpreference-service) stores the selected device.
- Pause/resume: an update to the user settings that sets the training-status-paused flag. Unknown: the exact field name.

## Training Load / Acute Load (incl. Load Focus)

### Navigation
- Entry: the load link row on Training Status, the Training Load / Acute Load card on My Day, and the readiness acute-load factor (via Training Status).
- Two modes, chosen by a feature/capability check that the caller can override:
  - **Advanced mode** (devices with acute/chronic load):
    - Title "Load". Three fixed tabs; swiping between tabs is disabled.
      1. "Load Focus": the default tab.
      2. "Exercise Load".
      3. "Training Load": the acute-load chart. A caller can open straight to this tab with a navigation-destination extra.
  - **Legacy mode:**
    - Title "7-Day Load". A single page with the same chart layout as the Training Load tab, based on 7-day load.
- Exercise Load and Training Load tabs:
  - Each holds an endless pager of 28-day windows ending today.
  - Swiping back moves the window back by 28 days.
  - The period line shows the date range.
- Load Focus: a single 28-day window (today − 27 through today). Paging is Unknown (inferred: none).
- Menu: "Help".
- Device selector: shown when more than one device recorded load.

### Charts
- **Training Load tab: acute-load chart.**
  - Type: line chart over 28 days.
  - One point = one calendar day.
  - X axis: days 0…27 of the window, with date labels. Y axis: load units.
  - Series:
    - "Optimal range" band: a filled area between the daily lower and upper chronic tunnel values. The upper fill uses an accent gradient; the lower fill uses the background colour, so only the band between them stays coloured.
    - Paused spans: shaded vertical areas over days marked paused.
    - "Acute Load": a line with dots, one value per day.
    - "Chronic Load": an optional second line with dots. It is shown only when the user turns on the "Chronic Load" toggle above the chart.
  - Y range:
    - Top = a rounded "nice" number at or above 1.1 × the largest plotted value.
    - Bottom = the smallest of acute, tunnel min, pause and chronic values. If that is 0, the bottom is set slightly below 0 so the line is not clipped.
  - Legend: optimal range, acute load and (when on) chronic load. The legend title is "4w timeline".
  - Scrubbing: touch-and-drag selects a day and draws a vertical highlight line. The header then shows that day's values (date, acute, chronic, status).
  - Colour roles: band = accent/positive tint; acute = primary series; chronic = secondary series; paused = neutral.
- **Training Load tab: load ratio view.**
  - A switch button toggles from the acute-load chart to a second chart of the acute:chronic ratio per day.
  - The ratio view has its own title, a value plus message line above the chart, a stats row and the same chronic toggle.
  - When the ratio data is flagged empty, the chart shows "no data available".
  - Ratio zones and colours: Unknown (inferred: they reuse the acwr status thresholds).
- **Load Focus tab:** three horizontal category bars.
  - Categories: "Anaerobic", "High Aerobic", "Low Aerobic".
  - Each bar shows the 4-week load in that category against a target range (min–max) drawn on the bar.
  - Colour roles: one fixed role per category (anaerobic, high aerobic, low aerobic). The target range is a marked span.
  - Exact bar geometry: Unknown (`LoadFocusView` not read; the three-bar layout is inferred from the three value/target pairs).
- **Exercise Load tab:** no chart (list only; see Lists).

### Summary numbers
- **Acute load** (header of the Training Load tab):
  - Value = `dailyTrainingLoadAcute` as an integer for the latest or selected day.
  - Legacy mode uses `weeklyTrainingLoad` instead.
  - Paused state: the header shows the value labelled as acute load and adds a "paused, resume to continue" message.
- **Load status qualifier:**
  - With the acute/chronic block, it comes from `acwrStatus`:

    | `acwrStatus` | Qualifier |
    |---|---|
    | LOW | Low |
    | OPTIMAL | Optimal |
    | HIGH | High |
    | VERY_HIGH | Very high |
    | NONE | Optimal |

    NONE mapping to Optimal is how the code reads; it is probably a fallback.
  - Without the block (legacy):
    - no value → no data;
    - above `loadTunnelMax` → High;
    - below `loadTunnelMin` → Low;
    - otherwise → Optimal.
- **Status message:** one of five feedback texts, chosen by `acwrStatusFeedback` (FEEDBACK_1 … FEEDBACK_5).
- **Chronic load:** `dailyTrainingLoadChronic` as an integer, shown when the toggle is on.
- **Ratio:** `dailyAcuteChronicWorkloadRatio`, a decimal. Its display precision is Unknown. `acwrPercent` is carried alongside it.
- **Load Focus:**
  - Window: today − 27 … today, shown as a date range.
  - Headline and description: from `trainingBalanceFeedbackPhraseKey`. Possible keys:
    - ON_TARGET, BALANCED, ABOVE_TARGETS, BELOW_TARGETS, NO_RESULT;
    - LOW_AEROBIC_OVERAGE, HIGH_AEROBIC_OVERAGE, ANAEROBIC_OVERAGE;
    - AEROBIC_LOW_SHORTAGE, AEROBIC_HIGH_SHORTAGE, ANAEROBIC_SHORTAGE;
    - AEROBIC_LOW_FOCUS, AEROBIC_HIGH_FOCUS, ANAEROBIC_FOCUS.
  - Each key has its own short headline and a longer explanation, written fresh in our own words.
  - Values per category: the 4-week load (integer) plus the target min and max.
  - "Has data" = any category load > 0.
  - When paused: a "paused since <date>" note.

### Lists
- **Exercise Load tab:** one row per activity in the 28-day window with load > 0.
  - Order: newest first (inferred from the comparator).
  - Each row shows:
    - the date;
    - the distance plus a short activity type ("Run", "Ride", "Swim", otherwise "Activity");
    - "Load: N";
    - "Benefit: <primary benefit>", e.g. Base, Tempo, Threshold, VO2 Max, Anaerobic, Sprint or Recovery;
    - a colour dot whose role follows the benefit category.
  - If an alternate load/benefit pair is present, it is used for multi-sport records (inferred).
  - Tapping a row opens the activity (inferred).
- **Device selector:** as on Training Status.

### Extra sections and empty states
- Help topics (one line each):
  - what acute load is (a decaying sum of recent exercise load);
  - what chronic load and the optimal range are;
  - how the load ratio is read;
  - what load focus categories mean;
  - how exercise load per activity is estimated.
- Empty states:
  - Training Load tab without data: a compact no-data panel replaces the chart.
  - Load ratio view with no data: the chart shows "no data available".
  - Exercise Load with no activities: a message saying there are no activities with load in this period.
  - Load Focus with all categories at 0: a no-data state. The headline key is NO_RESULT.
- Missing days: days without data have no acute point, so the line skips them (inferred: no interpolation). Band and pause series are only drawn where values exist.

### Data needed
- a server endpoint for the 28-day window. It gives per-day series:
  - `ACUTE_LOAD_DAILY_TRAINING`, `CHRONIC_LOAD_DAILY_TRAINING`;
  - `MIN_TRAINING_LOAD_CHRONIC`, `MAX_TRAINING_LOAD_CHRONIC`;
  - `DAILY_ACUTE_CHRONIC_WORKLOAD_RATIO`;
  - `ACUTE_LOAD_STATUS`, `ACUTE_LOAD_FEEDBACK`, `ACUTE_LOAD_PERCENTAGE`;
  - `TRAINING_STATUS_PAUSED`.
  It also gives `recordedDevices`.
- a server endpoint for the header values and for Load Focus:
  - `mostRecentTrainingLoadBalance.payload.metricsTrainingLoadBalanceDTOMap{<deviceId>}` with:
    - `monthlyLoadAerobicLow`, `monthlyLoadAerobicLowTargetMin`, `monthlyLoadAerobicLowTargetMax`;
    - `monthlyLoadAerobicHigh`, `…TargetMin`, `…TargetMax`;
    - `monthlyLoadAnaerobic`, `…TargetMin`, `…TargetMax`;
    - `trainingBalanceFeedbackPhraseKey`, `primaryTrainingDevice`, `deviceId`, `calendarDate`.
- Exercise load list: per-activity records with activity id, start timestamp, distance, activity type, device id, load, aerobic-benefit key, and an alternate load and benefit pair.
  - Unknown: the exact endpoint and JSON field names. The DTO fields are obfuscated as single letters, and the use case was not traced.
  - (inferred) Equivalent data is available from the activity list.

## VO2 Max

### Navigation
- Entry:
  - the VO2 Max card on My Day;
  - the VO2 link row on Training Status, which passes the sport;
  - performance stats.
- Title: "VO2 Max".
- Four tabs: "Current", "4w", "12w", "6m". The default is Current.
- Current: single page, no period header.
- 4w / 12w / 6m:
  - A previous/next header with arrows plus swipe paging.
  - Windows end today and cover 4 weeks, 12 weeks or 6 months; each step moves back one window.
  - The header shows the date range.
- Sport switch:
  - A segmented control ("Running", "Cycling", "Rowing") appears when more than one sport has data. With one sport, only that sport's name is shown.
  - The chosen sport applies to all tabs.
- A promo slot can appear at the top (out of scope).
- A caller can open the screen pre-set to a sport, or with a walking-test prompt.

### Charts
- **Current:** a semicircular gauge, no time axis.
  - The gauge spans 20 to 60 and the needle is clamped into that range.
  - Category arcs are not equal in length:
    - Poor (from 20 up to the first threshold) takes 40 % of the arc;
    - Fair, Good, Excellent and Superior take 15 % each.
  - Within an arc, the needle position is linear between that arc's two bounds.
  - Needle value: `vo2MaxPreciseValue`.
  - Colour roles: one per category, a scale from poor to superior.
- **4w:** line chart.
  - One point = one daily VO2 record (daily endpoint).
  - X = day offset within the window; labels are dates (thinning of daily labels inferred).
- **12w:** line chart.
  - One point = one weekly value (weekly endpoint).
  - About 12 evenly spaced week labels.
- **6m:** line chart.
  - One point = one weekly value (weekly endpoint).
  - 6 month labels.
- All three chart tabs:
  - Y = precise VO2 value in ml/kg/min.
  - Y axis runs from floor(min) to ceil(max) with about 5 ticks. The step is the first of 1, 2, 3, 5, 10, 20, 30, 40, 50 that fits.
  - Scrubbing shows a tooltip with the integer value and the date of the point.
  - Single series per sport; colour = primary series role.

### Summary numbers
- **Value:** integer of `vo2MaxValue`. Unit: ml/kg/min. The unit is shown in help and labels, not after the number.
- **Category:** from the precise value against age- and sex-specific thresholds t1…t4:
  - below t1 = Poor;
  - ≥ t1 Fair, ≥ t2 Good, ≥ t3 Excellent, ≥ t4 Superior.
  - Male or unspecified sex uses the male table.

  - Age bands: up to 29, 30–39, 40–49, 50–59, 60–69, 70 and over; separate cut-offs for men and women.
  - The app embeds a fixed table of four cut-offs per age band and sex that matches published
    population VO2 max norms (Cooper Institute style). Clean-room note: the numeric cut-offs are
    deliberately NOT reproduced here. The builder takes them from an openly published norm table
    and records that source in our code.

- **Percentile:**
  - For the user's age band and sex, the value is looked up in a 21-step table (5 % steps).
  - The result is shown as "top X %" when above the median and "bottom X %" otherwise.
  - The table values were not transcribed. (inferred) Rebuild them from the same public norms.
- **Fitness age:** shown when `fitnessAge` ≠ 0, as a short sentence with the age. It links to the fitness-age page.
- **Date:** "Today", "Yesterday" or the date of the latest record.
- **Heat/altitude flag:** `heatAltitudeAcclimation` comes with the sport record. Whether this screen shows it is Unknown.

### Lists
- None. Below each chart are the fitness-age sentence, the percentile sentence and a "Fitness Age" link.

### Extra sections and empty states
- Help topics (one line each):
  - what VO2 max is;
  - how to get an estimate (outdoor GPS run/ride with HR, or a walking test);
  - what the categories mean, with a link to the source norms.
- Empty states:
  - Current with no data: a "No VO2 max data" message, an optional "Get estimate" button (walking test), and the fitness-age link.
  - Chart tabs with no records in the window: "No data available" inside the chart area.
- Missing days: the line connects only existing points. There are no zero points (inferred).

### Data needed
- a server endpoint for the Current tab. It returns `{userId, generic, cycling, rowing, heatAltitudeAcclimation}`.
  - Each sport object carries `calendarDate`, `vo2MaxValue`, `vo2MaxPreciseValue`, `maxMetCategory`, `fitnessAge`, `fitnessAgeDescription`.
  - `generic` = running.
- a server endpoint for 4w.
- a server endpoint for 12w and 6m.
- The user profile's birth date and sex, for category and percentile.
- Not used here: an alternative `/maxmet/latest/hbase/{date}` path exists elsewhere.

## Recovery Time

### Navigation
- **Recovery Time has no page of its own.**
  - No activity, fragment, route or Compose destination with a recovery-time name was found.
  - No recovery-time endpoint is called by this app version.
- Where it lives:
  1. **Training Readiness:** a "Recovery Time" factor row. Tapping it opens a factor bottom sheet; see Training Readiness. The sheet's content for this factor is Unknown because did not decompile.
  2. Activity details and device screens can show the recovery time after an activity. That is outside this section.
- A My Day "Recovery Time" card, if Nimrag shows one, should open Training Readiness scrolled to the factor (recommended). Building a standalone page is a Nimrag design choice, not parity.

### Charts
- None in the app.

### Summary numbers
- Source: `recoveryTime` in minutes, from the training-readiness record.
- Display: hours = minutes / 60.
  - Up to 96 h: shown as "N h" (whole hours).
  - Above 96 h: shown in days. The leftover hours set the rounding:
    - remainder under 6 h → "N d";
    - 6–17 h → "N.5 d";
    - 18 h or more → (inferred) rounds up to "N+1 d". The decompiled string join is ambiguous.
- Qualifier, from `recoveryTimeFactorFeedback`: "High Need", "Moderate Need", "Low Need", "Fully Recovered".
- The factor contribution percentage `recoveryTimeFactorPercent` exists in the data. It is not shown on the readiness detail page.

### Lists
- None.

### Extra sections and empty states
- Missing or "NONE" feedback: the row shows "no data on device" and is disabled.

### Data needed
- a server endpoint: fields `recoveryTime`, `recoveryTimeFactorFeedback`, `recoveryTimeFactorPercent`.

## Weight

Scope: the detail screen opened from the Weight card on the home "My Day" screen, plus the
add-weigh-in flow, the delete flow and the goal entry point at a functional level.
Source build: Garmin Connect 5.31.1 (Android). The main screen composable and most of its
chart/list composables were not in the JADX output (missing classes); they were read from
the dex bytecode directly (method calls, constants, string ids), so layout order inside a
card is sometimes marked "(inferred)".

### Navigation

- Screen title: "Weight". Top bar with back arrow and an overflow menu.
- Range tabs (segmented row, shared framework): **1d, 7d, 4w, 1y** (interval types ONE_DAY,
  ONE_WEEK, FOUR_WEEKS, FIFTY_TWO_WEEKS).
- Default tab: **1d**. The UI state starts on ONE_DAY; a launcher may override the date
  (a launch flag) and the tab (a launch flag).
  - Tapping the My Day Weight card opens the screen on the **1d** tab, anchored on the
    **date of the last weigh-in** (the card's last-weight timestamp, or now if there is none).
  - Some other entry points (calendar, notifications) open it on 7d for a given date.
  - An extra flag can open the goal editor directly (a launch flag).
- Period movement: shared previous/next header plus horizontal paging of the period content.
  1d = one calendar day; 7d / 4w / 1y = rolling windows of 7 / 28 / 364 days ending on the
  anchor date, stepping back by the window length (same rules as the other report screens).
- Period line: shared header text (single date for 1d, date range otherwise).
- Cross-tab drill-down:
  - 7d / 4w: tapping a day row switches to the **1d** tab on that day.
  - 1y: tapping a week row switches to the **7d** tab anchored on that week's start date.
- Overflow menu items:
  - 1d tab: [unit choice: Pounds / Kilograms / Stones — only for users whose measurement
    system is "UK statute"], Add a weigh-in, Edit weight goal, Help, Delete a weigh-in
    (only when the selected day has at least one weigh-in).
  - Other tabs: [unit choice, UK only], Edit weight goal, Help.
  - Unit choice is stored as a local preference (0 = kg, 1 = lb (default), 2 = stone) and
    triggers a full reload of all three data sources.
  - Add a weigh-in opens the add flow pre-set to the selected day.
  - Edit weight goal navigates to an in-screen sub-route (goal editor).
  - Help opens the help hub for the Weight topic (see Extra sections).
- Optional extras on screen: a promotional banner slot at the top (scale promo) and a
  dismissible cross-sell card under the lists; a one-time "what's new" sheet for a newer
  scale model. Not needed for Nimrag (skip).

### Charts

No chart on 1d. 7d, 4w and 1y each show one chart card with an overlay selector.

Common chart card (7d / 4w / 1y):
- **Overlay chips** above or below the chart: single-select, tap again to clear. Options are
  BMI always, plus Body Fat %, Skeletal Muscle Mass, Bone Mass, Body Water % only when the
  period's average record contains a non-zero value for any of fat, muscle, bone or water.
  Order: BMI, Body Fat %, Muscle, Bone, Body Water %. The selection is persisted locally
  across sessions; if the saved overlay is not available in the current period it is not
  drawn. When the selection changes, the chip row scrolls to the selected chip.
- **Left Y axis** = weight (display unit). **Right Y axis** = overlay values (only when an
  overlay is selected). Default 4 labels per axis.
- **Y-axis range rule** (both axes): take all plotted values (points and both ends of every
  range bar / band). If there are none and no goal, the axis is 0..0.
  - With a goal > 0: pad = max(|max − goal|, |min − goal|) / 6;
    low = floor(min(min, goal) − pad), high = ceil(max(max, goal) + pad).
  - Without a goal: pad = ceil(max − min) / 6; low = floor(min − pad), high = ceil(max + pad).
  - Then make the span divisible by 3: span mod 3 = 1 → high+1 and low−1; mod 3 = 2 →
    high+1; span 0 → high+2 and low−1.
- **No data**: when weight points, overlay points, range bars and band are all empty, the
  chart shows a "No data" message.
- **Legend** (role → label): main series, high-low bars (7d/4w, only when present), overlay
  name (when selected), dashed goal line with the goal value and unit, pregnancy gain band
  (only when present).
- Colours as roles only: main weight series = primary data colour; high-low bars = neutral
  secondary; overlay = per-metric accent colour; goal = dashed line in an emphasis colour;
  pregnancy band = translucent fill.

7d and 4w (daily chart):
- X axis = one slot per calendar day from window start to window end (7 or 28 slots).
  Axis labels only at the first and last day (short date).
- **Main series** = line with point markers; one point per day = that day's **latest
  weigh-in** value in the display unit, rounded to 1 decimal. Days with no weigh-in or a
  zero value get no point.
- **High-low bars** (only when the multi-weigh-in feature is on, see Unknowns): for each day
  with min and max weight both > 0 and different, a rounded vertical bar from min to max.
- **Overlay series** on the right axis: one point per day from the same latest weigh-in
  (BMI, fat %, muscle, bone, water), rounded to 1 decimal; missing values skipped.
- **Goal line**: horizontal dashed line at the current weight goal, across the full width.
- **Pregnancy band**: for each day inside a goal range returned by the goal service, a
  low..high band (pregnancy weight-gain guidance). Only appears when such ranges exist.
- **Scrub/tap**: touching the chart selects the nearest day; a tooltip shows that day's date
  and, when present: low and high values, the last value, and the overlay value with its
  name and unit.

1y (weekly chart):
- X axis = one slot per 7-day step from window start (52 slots); month labels along the axis.
- **Main series** = line with points, one point per week = the server's **weekly average**
  (weight average of that week), rounded to 1 decimal; weeks without data skipped.
- **Weekly range bars** from the week's min to max weight (not gated by the feature flag).
- Overlay = weekly average of the chosen metric on the right axis.
- Goal line and pregnancy band as above, evaluated per week.
- Legend label for the main series reads as an average-weight label; no high-low legend
  entry on this tab.
- Scrub/tap: tooltip for the selected week (inferred to mirror the daily tooltip).

### Summary numbers

Units and conversion (applies everywhere):
- Server values: weight, muscle mass and bone mass in **grams**; body fat and body water in
  **percent**; BMI unitless. Goal value in grams.
- Display unit: metric users → kg; statute (US) users → lb; UK-statute users → their chosen
  unit (kg / lb / stone, default lb).
- kg = grams / 1000. lb = kg × 2.20462. stone = kg / 6.35 (i.e. grams / 6350).
- Rounding: value rounded half-even to 1 decimal, shown with thousands grouping and exactly
  one decimal (e.g. "72.4"). Stone is shown as whole stones + pounds, where pounds =
  fractional stone × 14 rounded to 1 decimal; long form "X st Y lbs", short form "X:Y".
- Missing value → "--".
- Unit label: kg / lbs / st.
- BMI fallback: when the record has no BMI (null or 0), compute weight_kg / (height_m)²
  using the profile height (stored in cm, divided by 100). No height → "--". For a single
  weigh-in coming from a Garmin Index scale with no BMI, no BMI is derived. BMI in weigh-in
  rows uses at most one decimal ("#.#").
- Percent values (fat, water) are shown only when > 0, with one decimal and a % sign.
- Trend of a change value: change converted to display unit; |change| < 0.05 → "trend
  maintained" (flat arrow); > 0 → up arrow; < 0 → down arrow. The arrow is an icon; the
  spoken description says up/down by the absolute amount.

1d tab (one day):
- **Hero value**: scale icon + the day's current weight (first entry of the day list as
  returned by the server — newest, inferred) in the display unit, followed by the unit.
  "--" when no weight.
- **Goal**: small line "Goal <value>" when a weight goal > 0 exists (goal-service).
- **Weigh-in count**: when the day has more than one weigh-in (multi-weigh-in feature on),
  a "N weigh-ins" caption.
- **Pregnancy info** (only for users flagged pregnant): current pregnancy week and the
  recommended low–high weight for that week. Optional for Nimrag.

7d / 4w tab — "Averages" card (from the range response's `totalAverage`):
- Row 1: **Weight** (average of the period, display unit, label carries the unit) and
  **BMI** (server average BMI; if missing/0 and at least one day's latest weigh-in did not
  come from an Index scale, computed from the average weight and profile height).
- Row 2 (shown when either value exists): **Body Fat %** (average, > 0 only) and
  **Skeletal Muscle Mass** (average, converted like weight).
- Row 3 (shown when either value exists): **Bone Mass** (average, converted) and
  **Body Water %** (average, > 0 only).
- These are server-computed period averages; the app does no averaging itself.

1y tab — same "Averages" card, filled from the weekly response's period-level average
object. BMI fallback here computes from the average weight whenever the
server BMI is missing/0 (no Index-scale check).

Not shown anywhere on this screen: min/max of the period as numbers, total change over the
period as a headline, goal remaining, metabolic age, visceral fat, physique rating (these
fields exist in the DTOs but are not displayed).

### Lists

1d tab — **weigh-in cards** (section heading "Weigh-ins" when the multi-weigh-in feature
is on):
- One card per weigh-in of the day, ordered **newest first** (sorted by the GMT timestamp,
  descending).
- Card heading: the weigh-in time (local time format) when it has a timestamp, otherwise
  "Weight <n>" with the position number.
- Rows (each only when the value exists):
  - Weight (value + unit).
  - Change: absolute difference to the previous weigh-in in display unit,
    with the trend arrow and trend description (order of Weight/Change rows inferred).
  - BMI (stored or computed; "#.#").
  - Body Fat % (> 0).
  - Skeletal Muscle Mass (converted).
  - Bone Mass (converted).
  - Body Water %.
- Rows are not tappable; no swipe actions. Deleting is done via the overflow menu.

7d / 4w tab — **daily rows** (one row per entry in `dailyWeightSummaries`, in server order;
whether days without data are returned is Unknown):
- Title: weekday name of the day.
- Subtitle: date ("MMM d, yyyy" style). With the multi-weigh-in feature on: if the day has
  more than one weigh-in, date + " • N weigh-ins"; otherwise date + " @ time" of the latest
  weigh-in.
- Trailing: the day's latest weight (with unit), and below it the absolute change
  (`weightDelta` of that latest weigh-in) with a small trend arrow; spoken text describes
  the trend.
- Tap → switches to the 1d tab for that day.

1y tab — **weekly rows** (one per entry in `weeklyWeightSummaries`, server order):
- Title: the week's date range (start – end).
- Trailing: week average weight with unit; below it the absolute week change
  with trend arrow.
- Tap → switches to the 7d tab for that week (anchored on the week start date).

Delete flow (overflow "Delete a weigh-in", 1d tab only):
- Multi-weigh-in mode: a bottom sheet titled "Delete a weigh-in" lists the day's weigh-ins
  (time and weight). Picking one opens a confirmation dialog naming the weigh-in time
  ("Delete <time> weigh-in?"), with Delete / Cancel. Confirm → delete that single weigh-in
  by its version id, then reload all data. Cancel clears the selection.
- Legacy mode (feature off): the confirmation handler deletes **all weights for the
  selected day** (by date). How the dialog is reached in this mode is Unknown (see below).
- After a delete the app also removes the matching record from Android Health Connect
  (not needed for Nimrag).

Add weigh-in flow (functional):
- Fields: Date (defaults to the day selected on the report, else today), Time (defaults to
  now), Weight (defaults to the profile weight).
- Time cannot be in the future: if the chosen date+time is after now, the time is clamped
  to the current hour/minute.
- Weight entry: a picker in the display unit (whole part + decimal; stone mode = stones +
  pounds); the value is converted to kg before saving (lb ÷ 2.20462, stone × 6.35).
- Save (top-bar action) posts one manual weigh-in: date (yyyy-MM-dd), local timestamp,
  GMT timestamp, source type MANUAL, unit "kg", value in kg. Body composition cannot be
  entered manually. On success the screen closes and the report refreshes (inferred).

Goal editor (functional, separate sub-screen): shows the current weight goal; save writes
the goal in grams as goal type "weight-grams"; delete removes the goal by its id.

### Extra sections and empty states

- **Help** (overflow → Help): opens a help hub with topics, one line each:
  - Weight — what body weight tracking shows.
  - BMI — what the body-mass index is.
  - Body Fat % — share of body mass that is fat.
  - Body Water % — share of body mass that is water.
  - Skeletal Muscle Mass — mass of skeletal muscle.
  - Bone Mass — estimated bone mineral mass.
- **1d empty state** (no weigh-in that day, also shown while loading/failed): card with a
  "No weight entered" title, a short hint that you can weigh in with a smart scale or enter
  a value by hand, the goal (if any), and a primary button "Add a weigh-in" (label "Add
  weight" in legacy mode) opening the add flow for that day.
- **7d / 4w / 1y**: chart shows "No data" when there is nothing to plot; the averages card
  shows "--" per missing value; the list is simply empty.
- Tab content (chart + averages + list) is only rendered once both the weight response and
  the goal response have loaded; if the goal call fails the tab stays blank (observed in
  code; probably unintended — Nimrag should render without a goal).
- Missing days: no point for the day (7d/4w) or week (1y); days/weeks are not interpolated.
  Whether the line bridges the gap visually is Unknown (chart library internals).

### Data needed

What the source app's server returns; Nimrag computes these from its own stored data.

| Purpose | Endpoint | Fields read |
|---|---|---|
| 1d list | GET a server endpoint → | `dateWeightList[]` (each: `weight`, `bmi`, `bodyFat`, `bodyWater`, `boneMass`, `muscleMass`, `sourceType`, `samplePk`, `timestampGMT` (epoch ms as string), `weightDelta`, `date`, `calendarDate`); also `startDate`, `endDate`, `totalAverage` (unused here) |
| 7d / 4w | GET a server endpoint → | `dailyWeightSummaries[]` (`summaryDate`, `numOfWeightEntries`, `minWeight`, `maxWeight`, `latestWeight` {same fields as a weigh-in}), `totalAverage` {`weight`, `bmi`, `bodyFat`, `bodyWater`, `boneMass`, `muscleMass`, counts `weightCount`…, `from`, `until`}, `previousDateWeight` (unused) |
| 1y | GET a server endpoint → | `weeklyWeightSummaries[]` (`startDate`, `endDate`, `averageWeight` {average record}, `minWeight`, `maxWeight`, `weightDelta`), (period average), (unused) |
| Goal + pregnancy band | GET a server endpoint → | `userGoals[]` (`goalValue` grams, `userGoalPk`, `startDate`, `endDate`), `userGoalRanges[]` (`startDate`, `endDate`, `lowWeightGoal`, `highWeightGoal` grams). For the 1d hero the app asks with today/today; charts use the visible window. |
| Add weigh-in | POST a server endpoint | body: `date`, `dateTimestamp`, `gmtTimestamp`, `sourceType`="MANUAL", `unitKey`="kg", `value` (kg) |
| Delete one weigh-in | DELETE a server endpoint | — |
| Delete all of a day | DELETE a server endpoint | — |
| Goal save / delete | PUT a server endpoint body {`goalValue` grams, type WEIGHT_GRAMS}; DELETE a server endpoint | — |
| Card (home) | GET a server endpoint | last weight, delta, timestamp (card only) |

Local data: profile height in cm (for BMI fallback), profile weight (add-flow default),
measurement system (metric / statute / UK statute), chosen weight unit (UK only), last
selected overlay.

Nimrag mapping hint: all of the above can be computed from stored weigh-ins (timestamp,
grams, body-composition fields, source): day list = weigh-ins of the day; daily summary =
latest / min / max / count per day; weekly = average per 7-day block; period averages =
mean of each field over weigh-ins in the window (server semantics for "average" — per
weigh-in or per day — Unknown).

Unknowns:
- Multi-weigh-in feature flag (`index2_mwpd`, also forced on for tester accounts): decides
  high-low bars on 7d/4w, the "N weigh-ins" captions, "@ time" subtitles and the per-
  weigh-in delete sheet. Its default in production is Unknown; Nimrag should behave as if
  it is on.
- Legacy delete path: with the flag off, the menu action only clears the pending selection
  in this build, so the "delete all for day" dialog is unreachable from that menu — Unknown
  whether intended.
- Server order of `dateWeightList`, `dailyWeightSummaries` and `weeklyWeightSummaries` is
  not re-sorted by the app (except the 1d cards); Unknown whether newest-first.
- Exact x position/extent of the goal line points (drawn as two points spanning the plot).
- How the server averages (per weigh-in vs per day) and how `weightDelta` is defined
  (change vs previous weigh-in, inferred).

## Summary table

| Screen | Tabs | Chart per tab | Summary numbers |
| --- | --- | --- | --- |
| Heart Rate | 1d, 7d, 4w, 1y | 1d: zone-gradient HR line 0-1440 min with alert markers and threshold lines; 7d/4w: daily High + Resting lines; 1y: weekly avg High + Resting lines | 1d: Resting, High; 7d/4w/1y: Avg Resting, Avg High |
| Stress | 1d, 7d, 4w, 1y | 1d: 3-min stress bars 0-1440 min (rest/stress/active roles, y -8..100); 7d/4w: daily overall-stress line 0..100; 1y: weekly avg line | 1d: Avg stress, qualifier, Rest/Low/Medium/High durations; 7d/4w/1y: Avg Stress Level |
| Body Battery | 1d, 7d, 4w | 1d: BB level line 0..100 over 0-1440 min (solid measured / dashed estimated) with optional Stress and Rest bar overlays; 7d/4w: daily low-to-high range bars | 1d: gauge (current or high/low), Charged, Drained, factors with impact; 7d/4w: none (list of daily High/Low) |
| Steps | 1d, 7d, 4w, 1y (weekly/monthly toggle) | 1d: ring + cumulative 15-min line with goal line; 7d/4w: daily bars coloured vs goal; 1y: 52 weekly or 12 monthly bars | percent of goal, distance, calories; total steps, total distance, avg daily, avg weekly/monthly |
| Floors | 1d, 7d, 4w, 1y | 1d: ring + mirrored cumulative climbed/descended area with goal line; 7d/4w: daily up/down bars + goal line; 1y: 52 weekly up/down bars | climbed, descended; totals and daily averages; weekly averages |
| Calories | 1d, 7d, 4w, 1y | 1d: donut resting vs active; 7d/4w: daily total bars with resting layer; 1y: weekly daily-average bars with resting layer | total, active, resting; averages (server for 7d/4w, client for 1y) |
| Intensity Minutes | 1d, 7d (calendar week), 4w, 1y | 1d: weekly running total through the day with dashed weekly-goal line + activity icons; 7d: weekly running total line; 4w: per-week running lines with per-week goals; 1y: 52 weekly bars coloured vs goal | moderate, vigorous, total (mod + 2x vig); weekly goal ring/percent/goal met; avg weekly moderate/vigorous, avg daily total |
| Sleep | 1 Day (Score / Coach sub-tabs), 7 Days, 4 Weeks, 1 Year | 1D: stage donut + stage timeline (hypnogram) with one selectable overlay; 7D/4W: score line 0–100 with overlay chips, duration-vs-need bars, bedtime/wake consistency segments (+ alignment page); 1Y: weekly avg score, weekly duration-vs-need bars, weekly consistency | Score /100 + feedback + insight; factors (duration, stress, deep, light, REM, awake, restlessness) with Excellent/Good/Fair/Poor from server; metrics grid (breathing variations, restless moments, overnight HR, RHR, BB change, SpO2 avg/low, respiration avg/low, overnight HRV, HRV status, skin temp); 7D/4W averages (score, duration, need, bed/wake, RHR, HR, BB, SpO2, resp, skin temp); 1Y Avg Score + Avg Duration per week |
| HRV Status | 1d, 7d, 4w (rolling) | 1d: status gauge + nightly 5-min line; 7d/4w: daily 7d-avg scatter with status icons + baseline band + optional overnight-avg dashed line | 1d: 7d Avg, status, Overnight Avg, Highest 5-min Avg; 7d/4w: mean Overnight Avg, mean 7d Avg (floor, ms) |
| Respiration | 1d, 7d, 4w (rolling) | 1d: hourly-avg line with High/Low range bars + Active bars (v200) or raw 2-min line (older); 7d/4w: two dotted lines, sleep avg and awake avg per day | 1d: Lowest, Highest, Awake Avg, Sleep Averages tiles (brpm, truncated); 7d/4w: Unknown |
| Pulse Ox | 1d, 7d, 4w (rolling) | 1d: band gauge + hourly cap-bars with spot-reading dots; 7d: hourly cap-bars across the week; 4w: daily cap-bars | 1d: shown value (latest today / avg otherwise), Avg SpO2 tile (today); 7d/4w: overall average SpO2 (%, truncated) |
| Training Readiness | none (single-day pager, default today) | score gauge 0–100 with level arcs; factor sheets (acute load chart vs optimal band, HRV, stress) | score, level, short/long feedback, updated time+context, 7 factors (dive, sleep score, recovery time, HRV, acute load, sleep history, stress history) |
| Training Status | Current, 4w, 12w, 6m (default Current) | Current: none (status block + link rows); 4w/12w/6m: segmented status bar + status/date table | status, since date, feedback, load focus headline, VO2 link, load link, HRV link, altitude/heat acclimation |
| Training Load | Load Focus, Exercise Load, Training Load (default Load Focus; legacy: single 7-day page) | Load Focus: 3 category bars with target ranges; Exercise Load: none (activity list); Training Load: 28-day acute-load line + optimal band + optional chronic line, switchable to load-ratio line | acute load, load status, feedback, chronic load, ratio, load focus headline + 3 category loads vs targets, per-activity load/benefit |
| VO2 Max | Current, 4w, 12w, 6m (default Current); sport switch Running/Cycling/Rowing | Current: category gauge 20–60; 4w: daily line; 12w: weekly line; 6m: weekly line | VO2 value, category, percentile, fitness age, date |
| Recovery Time | none (no own page; factor row inside Training Readiness) | none | recovery hours/days, need qualifier |
| Weight | 1d, 7d, 4w, 1y (default 1d; card opens 1d on last weigh-in date) | 1d: no chart (hero weight + weigh-in cards); 7d/4w: daily line of each day's latest weigh-in + high-low bars + optional overlay (BMI/fat/muscle/bone/water, right axis) + dashed goal line + pregnancy band; 1y: weekly-average line + weekly min-max bars + overlay + goal line | 1d: current weight, goal, weigh-in count, per weigh-in weight/change/BMI/fat%/muscle/bone/water%; 7d/4w/1y: Averages card (weight, BMI, fat %, muscle, bone, water %) + day/week rows with value and change trend |
