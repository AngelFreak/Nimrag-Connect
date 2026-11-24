import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { BackIcon, SampleDataChip, SeriesChart, ValueOrUnavailable, type CursorStore, type RefLine } from '../../components';
import type { AxisUnit } from '../../lib/ticks';
import { activityTitle } from './format';
import { SportDisc } from './SportDisc';
import './activities.css';

// Building blocks of the desktop activity pages (docs/design/web-layout/SPEC.md, D): two columns, a
// breadcrumb, a title with a sport disc, five headline figures, a stack of charts and groups of stats.

/** The kind as the breadcrumb says it ("Running"); other kinds keep their own name. */
const KIND_NOUNS: Record<string, string> = {
  run: 'Running',
  walk: 'Walking',
  hike: 'Hiking',
  cycle: 'Cycling',
  swim: 'Swimming',
};
export function kindNoun(a: { kind: string; sport_name: string | null }): string {
  return KIND_NOUNS[a.kind] ?? activityTitle(a);
}

/** Main column and side column (745 : 350 with a 24 px gap, shrinking together on a narrower window). */
export function DetailLayout({ main, side }: { main: ReactNode; side: ReactNode }) {
  return (
    <div className="adt">
      <div className="adt__main">{main}</div>
      <aside className="adt__side" aria-label="More about this activity">
        {side}
      </aside>
    </div>
  );
}

export interface Headline {
  /** What the figure is, so the Stats do not repeat it ("distance", "pace"). */
  id?: string;
  label: string;
  /** Pre-formatted; a missing value leaves the cell out. */
  value: string | null;
}

interface DetailHeaderProps {
  backTo: string;
  /** "Activities" or "Strength": the first breadcrumb. */
  section: string;
  /** "Running": the breadcrumb's second part, before "on <date>". */
  noun: string;
  when: string;
  kind: string;
  title: string;
  meta?: string | null;
  sample: boolean;
  headline: readonly Headline[];
  /** Under the title: the personal-record badge. */
  badge?: ReactNode;
  /** An earlier session to step to (only where the API names one). */
  prev?: { to: string; label: string } | null;
}

export function DetailHeader({ backTo, section, noun, when, kind, title, meta, sample, headline, badge, prev }: DetailHeaderProps) {
  const cells = headline.filter((h): h is Headline & { value: string } => h.value !== null && h.value !== '');
  return (
    <header className="adt-head">
      <div className="adt-head__crumbrow">
        <nav className="adt-head__crumb" aria-label="Breadcrumb">
          <Link to={backTo}>{section}</Link>
          <span aria-hidden="true">›</span>
          <span>
            {noun} · on {when}
          </span>
        </nav>
        {prev ? (
          <Link to={prev.to} className="adt-head__arrow" aria-label={prev.label} title={prev.label}>
            <BackIcon size={18} />
          </Link>
        ) : null}
      </div>
      <div className="adt-head__titlerow">
        <SportDisc kind={kind} size={56} tone="soft" />
        <div className="adt-head__titles">
          <div className="adt-head__line">
            <h1 className="adt-head__title">{title}</h1>
            <SampleDataChip sample={sample} />
          </div>
          {meta ? <p className="adt-head__meta">{meta}</p> : null}
          {badge ? <div className="adt-head__badge">{badge}</div> : null}
        </div>
      </div>
      {cells.length ? (
        <dl className="adt-head__stats">
          {cells.map((c) => (
            <div key={c.label} className="adt-head__stat">
              <dt>{c.label}</dt>
              <dd>{c.value}</dd>
            </div>
          ))}
        </dl>
      ) : null}
    </header>
  );
}

interface ChartBlockProps {
  /** Token colour of the dot, the line and the fill. */
  color: 'heart' | 'link' | 'energy' | 'steps' | 'warn';
  title: string;
  label: string;
  summary: string;
  x: number[];
  data: (number | null)[];
  xLabels: { value: number; text: string }[];
  unit?: AxisUnit;
  refLine?: RefLine;
  yReverse?: boolean;
  height?: number;
  /** The pointer line shared with the other charts of the stack. */
  cursor?: { group: string; store: CursorStore };
}

const DOT: Record<ChartBlockProps['color'], string> = {
  heart: 'var(--heart)',
  link: 'var(--link)',
  energy: 'var(--energy)',
  steps: 'var(--steps)',
  warn: 'var(--warn)',
};

/** One chart of the stack: a coloured dot and the title above, 120 px of chart, the average as a dashed line. */
export function ChartBlock({ color, title, label, summary, x, data, xLabels, unit, refLine, yReverse, height = 120, cursor }: ChartBlockProps) {
  return (
    <section className="adt-chart" aria-label={title}>
      <h3 className="adt-chart__title">
        <span className="adt-chart__dot" style={{ background: DOT[color] }} aria-hidden="true" />
        {title}
      </h3>
      <SeriesChart
        label={label}
        summary={summary}
        x={x}
        series={[{ label: title, color, data, area: true, width: 1.5 }]}
        height={height}
        xLabels={xLabels}
        {...(unit ? { yUnit: unit } : {})}
        {...(refLine ? { refLine } : {})}
        {...(yReverse ? { yReverse } : {})}
        {...(cursor ? { cursor } : {})}
      />
    </section>
  );
}

export interface StatGroup {
  title: string;
  items: { id?: string; label: string; value: string | null }[];
}

/** Groups of figures in two columns: a title with a rule, then value over label; figures without a value are left out. */
export function StatGroups({ groups }: { groups: readonly StatGroup[] }) {
  const shown = groups
    .map((g) => ({ ...g, items: g.items.filter((i) => i.value !== null && i.value !== '') }))
    .filter((g) => g.items.length > 0);
  if (shown.length === 0) {
    return (
      <p>
        <ValueOrUnavailable value={null} />
      </p>
    );
  }
  return (
    <div className="adt-groups">
      {shown.map((g) => (
        <section key={g.title} className="adt-group">
          <h3 className="adt-group__title">{g.title}</h3>
          <dl className="adt-group__items">
            {g.items.map((i) => (
              <div key={i.label} className="adt-group__item">
                <dt>{i.label}</dt>
                <dd>{i.value}</dd>
              </div>
            ))}
          </dl>
        </section>
      ))}
    </div>
  );
}

/** A card of the side column: a small title, then its content. */
export function SideCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="adt-side-card">
      <h2 className="adt-side-card__title">{title}</h2>
      {children}
    </section>
  );
}

/** Label and value rows inside a side card. */
export function SideFacts({ items }: { items: readonly { label: string; value: string | null; sub?: string }[] }) {
  const shown = items.filter((i) => i.value !== null && i.value !== '');
  return (
    <dl className="adt-facts">
      {shown.map((i) => (
        <div key={i.label} className="adt-facts__row">
          <dt>{i.label}</dt>
          <dd>
            {i.value}
            {i.sub ? <span className="adt-facts__sub">{i.sub}</span> : null}
          </dd>
        </div>
      ))}
    </dl>
  );
}
