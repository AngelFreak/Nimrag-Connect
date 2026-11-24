# Polish B: personal records (2026-11-24)

Goal: a Personal Records page, computed by the server from the person's own activities.

**Records** (per sport group):
- Running (all running sub-sports, incl. treadmill only when distance is from the watch): fastest
  400 m, 1 km, 1 mile, 5 km, 10 km, half marathon (21.0975 km), marathon (42.195 km); longest run
  (distance); most ascent in a run.
- Walking/hiking: longest (distance), most ascent.
- Cycling: longest ride, fastest 20 km, fastest 40 km, most ascent.
- All sports: longest activity by time.
- Steps (daily totals): most steps in a day, most steps in a week (Mon–Sun).

**Best efforts** are the fastest time over any continuous stretch of the given distance inside one
activity (not only whole activities): a two-pointer sliding window over the activity's records
(cumulative distance, timestamp), interpolating the time at the exact distance. Skip stretches with
a gap in records over 30 s (pauses). Only activities whose total distance covers the effort.

**Server:** compute on demand with a cache table keyed by activity sha256 (best efforts of one
activity are computed once, when it is derived or first asked for; re-derive clears it). `GET
/api/records` → per record: kind, value (seconds or metres or steps), the activity it came from
(id, name/sport, date) or the date for step records, plus the previous best (value, date) when
there is one. `GET /api/activities/{id}` gains `records: [kind]` = the records this activity set
(so the activity page can show a "Personal record" badge with what it set). Only the signed-in
user's data. ts-rs types.

**Web:** `/records` page: sections per sport group, each record a row with the value (time as
h:mm:ss or m:ss, distance in km with 2 decimals, ascent in m, steps with thousands separators), the
date and a link to the activity. Empty state: "No runs yet" etc. Linked from the Activities page
header ("Records") and from the activity page badge. Desktop: two columns. Add to
`web/e2e/routes.ts`. The activity page shows the badge ("Personal record: 5 km, 10 km") near the
title.

Tests: server tests with synthetic records (exact interpolation, pause skip, distance shorter than
the effort, user isolation, cache invalidation on re-derive); web tests for formatting and empty
states.
