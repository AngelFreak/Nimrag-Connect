import { useState, type ReactNode } from 'react';
import {
  BarList,
  BarSeries,
  BottomNav,
  Button,
  Card,
  CardHeader,
  CheckRow,
  Chip,
  DataList,
  DesktopNav,
  EmptyState,
  ErrorState,
  FilterChips,
  Legend,
  ListRow,
  MapView,
  ProfileMenu,
  ProgressBar,
  RangeBar,
  Ring,
  SampleDataChip,
  SegmentedControl,
  SeriesChart,
  Skeleton,
  StageBar,
  StageTimeline,
  StatTile,
  StressBars,
  SyncButton,
  Table,
  TextField,
  TopBar,
  Unavailable,
  WatchLink,
  ZoneBars,
  type StageSpan,
  type StressBucket,
} from '../components';
import { usePageTitle } from '../lib/hooks';
import type { LinkState } from '../native';

// Component gallery for design review at /__gallery. Every component in its states, with
// the sample values of the design canvas. Static: nothing here talks to the API.

const range = (n: number): number[] => Array.from({ length: n }, (_, i) => i);
const HR_24H = [62, 58, 52, 49, 48, 50, 60, 72, 68, 76, 72, 82, 134, 148, 118, 86, 74, 68];
const HRV_30 = [60, 64, 57, 62, 66, 59, 55, 63, 65, 58, 54, 61, 64, 60, 56, 62, 63, 57, 48, 44, 52, 59, 63, 65, 58, 61, 66, 62, 59, 62];
const ENERGY_DAY = [20, 48, 74, 94, 90, 84, 76, 62, 50, 44, 38, 40, 42];
const SLEEP_7 = [385, 412, 365, 399, 450, 372, 432];

function Section({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="gallery__section" aria-labelledby={`g-${title.replace(/\W+/g, '-')}`}>
      <h2 className="gallery__heading" id={`g-${title.replace(/\W+/g, '-')}`}>
        {title}
      </h2>
      {note ? <p className="gallery__note">{note}</p> : null}
      {children}
    </section>
  );
}

function Frame({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="gallery__frame">
      <div className="gallery__label">{label}</div>
      {children}
    </div>
  );
}

const COLORS: [string, string][] = [
  ['bg', '#0F1417'],
  ['card', '#171E22'],
  ['raised', '#12181C'],
  ['line', '#243037'],
  ['line-strong', '#2A353B'],
  ['text', '#E8EDEE'],
  ['text-2', '#9AA7AC'],
  ['link', '#7DBBFF'],
  ['energy', '#4FD1B5'],
  ['sleep', '#A594FF'],
  ['heart', '#FF6B6B'],
  ['steps', '#F2D45C'],
  ['warn', '#FFB066'],
  ['zone-1', '#5B8DBE'],
  ['zone-2', '#4FD1B5'],
  ['zone-3', '#F2D45C'],
  ['zone-4', '#FFA24D'],
  ['zone-5', '#FF6B6B'],
];

const SPANS: StageSpan[] = [
  { stage: 'light', from: 0, to: 25 },
  { stage: 'deep', from: 25, to: 85 },
  { stage: 'light', from: 85, to: 127 },
  { stage: 'rem', from: 127, to: 158 },
  { stage: 'light', from: 158, to: 205 },
  { stage: 'awake', from: 205, to: 212 },
  { stage: 'light', from: 212, to: 300 },
  { stage: 'rem', from: 300, to: 380 },
  { stage: 'light', from: 380, to: 444 },
];

const STRESS: StressBucket[] = [
  { level: 'rest', value: null, span: 6 },
  { level: 'low', value: 30 },
  { level: 'low', value: 40 },
  { level: 'low', value: 24 },
  { level: 'medium', value: 56 },
  { level: 'medium', value: 64 },
  { level: 'activity', value: null, span: 2 },
  { level: 'low', value: 30 },
  { level: 'rest', value: null, span: 2 },
];

const ROWS = [
  { id: 'a', date: 'Wed 18 Nov', type: 'Strength', dur: '42 min', hr: '112' },
  { id: 'b', date: 'Mon 16 Nov', type: 'Run', dur: '38 min', hr: '151' },
  { id: 'c', date: 'Fri 13 Nov', type: 'Walk', dur: '52 min', hr: null },
];

const LINKS: LinkState[] = ['connected', 'waiting', 'problem', 'unknown'];

export function GalleryPage() {
  usePageTitle('Component gallery');
  const [range7, setRange7] = useState<'7d' | '4w' | '3m' | '1y'>('4w');
  const [type, setType] = useState<'all' | 'run' | 'strength' | 'walk'>('all');
  const [spinDemo, setSpinDemo] = useState(false);

  return (
    <div className="gallery">
      <header>
        <h1 className="topbar__title" style={{ fontSize: 30 }}>
          Component gallery
        </h1>
        <p className="gallery__note">Every component in its states, with the sample values of the design canvas. Static review page, not part of the app navigation.</p>
      </header>

      <Section title="Design tokens" note="Colours from the design canvas; type is Inter (text) and Space Grotesk (titles and numbers), both self-hosted.">
        <div className="gallery__grid">
          {COLORS.map(([name, hex]) => (
            <div key={name} style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
              <span style={{ width: 28, height: 28, borderRadius: 8, background: hex, border: '1px solid var(--line-strong)' }} aria-hidden="true" />
              <span style={{ fontSize: 13 }}>
                --{name} <span className="tone-muted">{hex}</span>
              </span>
            </div>
          ))}
        </div>
        <Card>
          <span style={{ fontFamily: 'var(--font-display)', fontSize: 34, fontWeight: 600 }}>Space Grotesk 34 / 600: 7h 12m</span>
          <span style={{ fontFamily: 'var(--font-display)', fontSize: 22, fontWeight: 600 }}>Space Grotesk 22 / 600: Sleep</span>
          <span style={{ fontSize: 16, fontWeight: 600 }}>Inter 16 / 600: Strength, 42 min</span>
          <span style={{ fontSize: 14 }}>Inter 14 / 400: Everything stays on your server.</span>
          <span className="tone-muted" style={{ fontSize: 13 }}>
            Inter 13 / 400 muted: 6 exercises · 18 sets
          </span>
          <span className="tone-muted" style={{ fontSize: 12 }}>
            Inter 12 / 400 muted: Wed 18 Nov · 18:05
          </span>
        </Card>
      </Section>

      <Section title="App shell and navigation" note="This page itself runs inside AppShell: on a phone it has the top bar and bottom navigation, from 1024 px the dashboard header. The pieces on their own:">
        <div className="gallery__grid">
          <Frame label="Top bar of the main tabs (profile, sync, watch)">
            <div className="mainbar" style={{ padding: 8 }}>
              <ProfileMenu align="start" />
              <div className="mainbar__right">
                <SyncButton spinning={false} onClick={() => setSpinDemo(true)} />
                <WatchLink link="connected" />
              </div>
            </div>
          </Frame>
          <Frame label="Bottom navigation, Activities active">
            <BottomNav active="activities" />
          </Frame>
        </div>
        <Frame label="Desktop navigation, Overview active">
          <div style={{ padding: '0 16px' }}>
            <DesktopNav active="today" />
          </div>
        </Frame>
        <div className="gallery__grid">
          <Frame label="Watch icon states (dot shape plus text in the accessible name)">
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, padding: 8, alignItems: 'center' }}>
              {LINKS.map((l) => (
                <div key={l} style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', fontSize: 12 }}>
                  <WatchLink link={l} />
                  <span className="tone-muted">{l}</span>
                </div>
              ))}
            </div>
          </Frame>
          <Frame label="Sync button: idle, spinning, in the Android app (Sync now)">
            <div style={{ display: 'flex', gap: 16, padding: 8, alignItems: 'center' }}>
              <div style={{ textAlign: 'center', fontSize: 12 }}>
                <SyncButton spinning={spinDemo} onClick={() => setSpinDemo((s) => !s)} />
                <div className="tone-muted">{spinDemo ? 'spinning' : 'idle'} (click)</div>
              </div>
              <div style={{ textAlign: 'center', fontSize: 12 }}>
                <SyncButton spinning native onClick={() => {}} />
                <div className="tone-muted">spinning, native</div>
              </div>
            </div>
          </Frame>
        </div>
      </Section>

      <Section title="Top bars">
        <div className="gallery__grid">
          <Frame label="Large title with subtitle">
            <TopBar variant="large" headingLevel={2} subtitle="Thursday 19 November" title="Today" aside={<><SampleDataChip sample /><span>Watch synced 07:42</span></>} />
          </Frame>
          <Frame label="Back">
            <TopBar variant="back" headingLevel={2} backTo="/" title="Sleep" subtitle="Night of Wed 18 Nov" aside={<SampleDataChip sample />} />
          </Frame>
          <Frame label="Previous and next, centred title">
            <TopBar variant="pager" headingLevel={2} title="Wed 18 Nov" subtitle="Day view · sample data" prevLabel="Previous day" nextLabel="Next day" prev={{ to: '/__gallery' }} next={{ to: '/__gallery' }} />
          </Frame>
          <Frame label="Previous and next, end of range (next unavailable)">
            <TopBar variant="pager" headingLevel={2} title="Thu 19 Nov" subtitle="Day view" prevLabel="Previous day" nextLabel="Next day" prev={{ to: '/__gallery' }} next={null} />
          </Frame>
        </div>
      </Section>

      <Section title="Card and stat tile">
        <div className="gallery__grid">
          <StatTile label="Sleep" value="7h 12m" sub="Score 81" tone="sleep" />
          <StatTile label="Overnight HRV" value="62 ms" sub="Within your range" tone="link" />
          <StatTile label="Resting heart rate" value="51 bpm" sub="7-day average 52" />
          <StatTile label="Energy now" value="68" sub="+54 charged overnight" tone="energy" size="xl" />
          <StatTile label="Steps" value="6,240" sub="Goal 8,000">
            <ProgressBar value={6240} max={8000} label="6,240 of 8,000 steps" />
          </StatTile>
          <StatTile label="Overnight HRV" value={null} sub="No reading last night" />
          <StatTile size="sm" label="Distance" value="6.4 km" />
          <StatTile size="sm" label="Pace" value={undefined} />
          <Card>
            <CardHeader title="Card with header" aside="aside text" />
            <p className="tone-muted">Plain card, 16 px radius.</p>
          </Card>
        </div>
      </Section>

      <Section title="Unavailable" note="Hard rule: a missing metric reads “unavailable”, never 0, a dash or blank.">
        <Card row>
          <span>Resting heart rate</span>
          <Unavailable />
        </Card>
      </Section>

      <Section title="Ring, progress bars">
        <div className="gallery__grid">
          <Card row>
            <Ring value={68} label="Energy level" />
            <Ring value={61} label="Recovery complete" color="var(--accent)" size={72} strokeWidth={8} showValue={false} />
            <Ring value={null} label="Energy level" />
          </Card>
          <Card padding="lg">
            <CardHeader title="ProgressBar" aside="78%" />
            <ProgressBar value={6240} max={8000} height={8} label="6,240 of 8,000 steps, 78 percent" />
            <ProgressBar value={null} max={8000} height={8} label="Steps" />
            <RangeBar value={312} max={600} target={[240, 420]} label="Load of 312 sits inside the target range of 240 to 420" />
          </Card>
        </div>
      </Section>

      <Section title="Series chart (uPlot)" note="Labelled image plus a text summary for screen readers. All-null data shows “unavailable”.">
        <div className="gallery__grid">
          <Card>
            <CardHeader title="Overnight HRV" aside="Avg 61 ms · range 55 to 70" />
            <SeriesChart label="Overnight HRV over 4 weeks" summary="Overnight HRV over 4 weeks, mostly inside the usual range of 55 to 70 milliseconds with one dip in the third week." x={range(HRV_30.length)} series={[{ label: 'HRV', color: 'link', data: HRV_30 }]} band={[55, 70]} height={120} axes />
          </Card>
          <Card>
            <CardHeader title="Heart rate, last 24 hours" aside="48 to 148 bpm" tone="muted" />
            <SeriesChart label="Heart rate over the last 24 hours" summary="Heart rate over the last 24 hours, low overnight with a peak of 148 bpm." x={range(HR_24H.length)} series={[{ label: 'Heart rate', color: 'heart', data: HR_24H }]} height={96} xLabels={[0, 6, 12, 18, 24].map((v) => ({ value: (v / 24) * (HR_24H.length - 1), text: String(v).padStart(2, '0') }))} />
          </Card>
          <Card>
            <CardHeader title="Energy" aside="High 94 · Low 38" />
            <SeriesChart label="Energy across the day" summary="Energy charges overnight to 94 and drains to 38 after the evening workout." x={range(ENERGY_DAY.length)} series={[{ label: 'Energy', color: 'energy', data: ENERGY_DAY, area: true }]} yRange={[0, 100]} height={100} />
          </Card>
          <Card>
            <CardHeader title="Sparkline in a tile" />
            <SeriesChart label="Breathing rate" summary="Breathing rate overnight, between 13.1 and 13.9 breaths per minute." x={range(8)} series={[{ label: 'Breathing', color: 'link', data: [13.4, 13.1, 13.6, 13.3, 13.9, 13.5, 13.2, 13.4], width: 2 }]} height={36} yUnit="decimal" sparkline />
          </Card>
          <Card>
            <CardHeader title="Gap in the data" />
            <SeriesChart label="Series with a gap" summary="A series with a gap: missing readings leave a break in the line." x={range(10)} series={[{ label: 'Gap', color: 'sleep', data: [60, 62, 61, null, null, null, 64, 66, 65, 63] }]} height={64} />
          </Card>
          <Card>
            <CardHeader title="No data at all" />
            <SeriesChart label="Empty series" summary="No data." x={range(5)} series={[{ label: 'None', color: 'link', data: [null, null, null, null, null] }]} />
          </Card>
        </div>
      </Section>

      <Section title="Bars: sleep, stress, zones">
        <div className="gallery__grid">
          <Card padding="lg">
            <CardHeader title="Stages through the night" tone="muted" />
            <StageTimeline spans={SPANS} totalMinutes={444} label="Sleep stages: deep sleep early, REM growing toward morning, one awakening" ticks={['23:18', '01:00', '03:00', '05:00', '06:42']} />
            <Legend
              grid
              items={[
                { key: 'deep', label: 'Deep', color: 'var(--stage-deep)', value: '1h 24m' },
                { key: 'light', label: 'Light', color: 'var(--stage-light)', value: '3h 58m' },
                { key: 'rem', label: 'REM', color: 'var(--stage-rem)', value: '1h 50m' },
                { key: 'awake', label: 'Awake', color: 'var(--stage-awake)', value: null },
              ]}
            />
          </Card>
          <Card padding="lg">
            <CardHeader title="Stacked bar (intensity minutes)" aside="Weekly goal 150" />
            <StageBar total={150} label="96 of 150 intensity minutes: 52 moderate and 44 from vigorous activity" segments={[{ key: 'm', label: 'Moderate', value: 52, color: 'var(--zone-1)' }, { key: 'v', label: 'Vigorous', value: 44, color: 'var(--warn)' }]} />
            <Legend items={[{ key: 'm', label: 'Moderate 52', color: 'var(--zone-1)' }, { key: 'v', label: 'Vigorous 22, counted double', color: 'var(--warn)' }]} />
            <StageBar label="No data" segments={[{ key: 'm', label: 'Moderate', value: null, color: 'var(--zone-1)' }]} />
          </Card>
          <Card>
            <CardHeader title="Stress" aside="Average 28 · mostly low" />
            <StressBars buckets={STRESS} label="Stress level across the day: resting overnight, low through the morning, a medium stretch mid-afternoon" />
            <Legend
              items={[
                { key: 'rest', label: 'Rest 9h 10m', color: 'var(--stress-rest)' },
                { key: 'low', label: 'Low 10h 30m', color: 'var(--stress-low)' },
                { key: 'medium', label: 'Medium 2h 10m', color: 'var(--stress-medium)' },
                { key: 'activity', label: 'Activity', color: 'var(--stress-activity)', outlined: true },
              ]}
            />
          </Card>
          <Card padding="lg">
            <CardHeader title="Time in heart rate zones" tone="muted" />
            <ZoneBars zones={[{ zone: 5, minutes: 1 }, { zone: 4, minutes: 8 }, { zone: 3, minutes: 18 }, { zone: 2, minutes: 9 }, { zone: 1, minutes: 2 }]} />
          </Card>
          <Card padding="lg">
            <CardHeader title="Zones without heart-rate data" tone="muted" />
            <ZoneBars zones={[{ zone: 2, minutes: null }, { zone: 1, minutes: undefined }]} />
          </Card>
          <Card padding="lg">
            <CardHeader title="Load by intensity, 4 weeks" />
            <BarList label="Load by intensity" max={900} items={[{ key: 'e', label: 'Easy aerobic', value: 412, color: 'var(--stress-rest)' }, { key: 'h', label: 'Hard aerobic', value: 556, color: 'var(--accent)' }, { key: 'a', label: 'Anaerobic', value: 128, color: 'var(--warn)' }]} />
          </Card>
          <Card padding="lg">
            <CardHeader title="Last 7 nights" tone="muted" aside="Average 6h 58m" />
            <BarSeries values={SLEEP_7} labels={['Thu', 'Fri', 'Sat', 'Sun', 'Mon', 'Tue', 'Wed']} highlightIndex={6} label="Sleep duration for the last 7 nights, between 6 hours 5 minutes and 7 hours 30 minutes" />
          </Card>
          <Card padding="lg">
            <CardHeader title="Steps this week" aside="Avg 7,800 a day" />
            <BarSeries values={[9120, 7480, 8360, 6240, null, null, null]} labels={['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']} goal={8000} max={10000} color="var(--steps)" belowGoalColor="var(--steps-dim)" height={100} label="Steps per day this week: Monday 9,120, Tuesday 7,480, Wednesday 8,360, Thursday 6,240 so far; the goal line is 8,000" />
          </Card>
        </div>
      </Section>

      <Section title="Chips and controls">
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <Chip>Chip</Chip>
          <SampleDataChip sample />
          <span className="tone-muted">(SampleDataChip with sample=false renders nothing: </span>
          <SampleDataChip sample={false} />
          <span className="tone-muted">)</span>
        </div>
        <FilterChips label="Filter by type" options={[{ value: 'all', label: 'All' }, { value: 'run', label: 'Run' }, { value: 'strength', label: 'Strength' }, { value: 'walk', label: 'Walk' }]} value={type} onChange={setType} />
        <div>
          <SegmentedControl label="Time range" options={[{ value: '7d', label: '7 days' }, { value: '4w', label: '4 weeks' }, { value: '3m', label: '3 months' }, { value: '1y', label: '1 year' }]} value={range7} onChange={setRange7} />
          <p className="gallery__note" style={{ marginTop: 8 }}>
            Selected: {range7}. Tab to the group, arrow keys move focus, Enter or Space selects.
          </p>
        </div>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <Button variant="primary" size="lg">Primary</Button>
          <Button size="lg">Secondary</Button>
          <Button variant="dashed" size="lg">Add step</Button>
          <Button variant="primary" disabled>Disabled</Button>
        </div>
        <div className="gallery__grid">
          <TextField label="Name" defaultValue="Upper body A" />
          <TextField label="Password" type="password" hint="Hint text under a field." />
          <Card padding="list">
            <CheckRow label="Time and time zone" defaultChecked />
            <CheckRow label="Weather" />
          </Card>
        </div>
      </Section>

      <Section title="Data list, table, list row">
        <div className="gallery__grid">
          <Card padding="lg">
            <CardHeader title="Calories today" />
            <DataList items={[{ label: 'Active', value: '412 kcal' }, { label: 'Resting', value: '1,780 kcal' }, { label: 'Firmware', value: null }, { label: 'Total', value: '2,192 kcal', strong: true }]} />
          </Card>
          <Card padding="lg">
            <CardHeader title="Recent activities" />
            <Table caption="Recent activities" rows={ROWS} rowKey={(r) => r.id} columns={[{ key: 'd', header: 'Date', cell: (r) => r.date }, { key: 't', header: 'Type', cell: (r) => r.type }, { key: 'du', header: 'Duration', cell: (r) => r.dur }, { key: 'hr', header: 'Avg HR', cell: (r) => r.hr, align: 'end' }]} />
          </Card>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            <ListRow to="/activities/1" title="Strength" sub="Wed 18 Nov · 18:05" value="42 min" valueSub="18 sets · 3,300 kg" />
            <ListRow to="/activities/2" title="Run" sub="Mon 16 Nov · 17:40" value="6.4 km" valueSub="38:00 · 5:56 /km" />
            <ListRow to="/activities/3" title="Walk" sub="Fri 13 Nov · 12:10" value={null} valueSub="52 min" />
          </div>
        </div>
      </Section>

      <Section title="States">
        <div className="gallery__grid">
          <EmptyState title="No activities yet" action={<Button variant="primary">Sync the watch</Button>}>Activities from the watch appear here after the next sync.</EmptyState>
          <ErrorState title="Could not load this page" action={<Button>Try again</Button>}>The server did not answer. Showing nothing is better than showing a wrong number.</ErrorState>
          <Card>
            <Skeleton height={14} width="40%" />
            <Skeleton height={28} width="60%" />
            <Skeleton height={64} />
            <Skeleton lines={3} height={12} />
          </Card>
        </div>
      </Section>

      <Section title="Map" note="Tiles come from /tiles/*.pmtiles on your own server. Without a tile file the design's placeholder shows; no third-party tile server is ever contacted.">
        <Card padding="flush">
          <MapView />
        </Card>
      </Section>
    </div>
  );
}
