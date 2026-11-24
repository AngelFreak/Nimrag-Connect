import { useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { Link, useLocation, useNavigate, useParams } from 'react-router-dom';
import { api } from '../api/client';
import type { CalendarResponse } from '../api/generated';
import { Card, CardHeader, DataList, LinkButton, PageHeader, SampleDataChip, StatStrip, TopBar } from '../components';
import { addMonths, browserToday, fmtMonth, fmtShortDay, isIsoMonth } from '../lib/dates';
import { fmtInt } from '../lib/format';
import { useIsDesktop, usePageTitle } from '../lib/hooks';
import { DayToolbar, StatsColumns, StatsSection } from './health/desktop';
import { activityRow, activityWords, CALENDAR_KEYS, dayMarkers, daysOfMonth, energyText, leadingBlanks, moveFocus, sleepText } from './body/calendar';
import { PageError, PageSkeleton, useOwnerToday } from './body/common';
import type { CalendarDay } from './body/types';
import './body/calendar.css';

// Stage 3l: GET /api/calendar/{yyyy-mm}. The month is in the URL (/calendar/2026-11, none means
// the current month). Every day is a link to /day/{date}; the grid is one tab stop and moves with
// the arrow keys (Page Up / Page Down change the month). Cardio is a circle, strength a square.

const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

export function CalendarPage() {
  const { month: param } = useParams();
  const ownerToday = useOwnerToday();
  const today = ownerToday ?? browserToday();
  const paramOk = isIsoMonth(param);
  // Without a month in the URL, wait until the owner's own "today" is known.
  const month = paramOk ? param : ownerToday?.slice(0, 7);
  const title = month ? fmtMonth(month) : 'Calendar';
  usePageTitle(month ? `Calendar ${title}` : 'Calendar');

  const query = useQuery({
    queryKey: ['calendar', month],
    queryFn: ({ signal }) => api.get<CalendarResponse>(`/api/calendar/${month}`, { signal }),
    enabled: month !== undefined,
  });
  const data = query.data;
  const desktop = useIsDesktop();
  const navigate = useNavigate();
  return (
    <>
      {desktop ? (
        <PageHeader title="Calendar" actions={data?.sample ? <SampleDataChip sample /> : undefined}>
          <DayToolbar
            label={title}
            prevLabel="Previous month"
            nextLabel="Next month"
            onPrev={() => month && void navigate(`/calendar/${addMonths(month, -1)}`)}
            onNext={() => month && void navigate(`/calendar/${addMonths(month, 1)}`)}
            atEnd={false}
          />
        </PageHeader>
      ) : (
        <TopBar
          variant="pager"
          title={title}
          subtitle={data?.sample ? 'Calendar · sample data' : 'Calendar'}
          prevLabel="Previous month"
          nextLabel="Next month"
          prev={month ? { to: `/calendar/${addMonths(month, -1)}` } : null}
          next={month ? { to: `/calendar/${addMonths(month, 1)}` } : null}
        />
      )}
      {query.isError ? (
        <PageError title="The calendar could not be loaded" error={query.error} onRetry={() => void query.refetch()} />
      ) : !data || !month ? (
        <PageSkeleton cards={2} />
      ) : (
        <CalendarBody key={month} month={month} data={data} today={today} />
      )}
    </>
  );
}

export function CalendarBody({ month, data, today }: { month: string; data: CalendarResponse; today: string }) {
  const navigate = useNavigate();
  const location = useLocation();
  const grid = useRef<HTMLDivElement>(null);
  const days = daysOfMonth(month);
  const byDate = new Map<string, CalendarDay>(data.days.map((d) => [d.date, d]));
  const monthName = fmtMonth(month).split(' ')[0];
  // A key press that crossed into this month names the day to focus in the navigation state.
  const carried = (location.state as { focus?: string } | null)?.focus;
  const arrivedAt = carried && carried.startsWith(month) ? carried : null;
  const [focusDay, setFocusDay] = useState(arrivedAt ?? (today.startsWith(month) ? today : `${month}-01`));

  useEffect(() => {
    if (arrivedAt) grid.current?.querySelector<HTMLElement>(`[data-date="${arrivedAt}"]`)?.focus();
  }, [arrivedAt]);

  // Arrow keys move between the days; Page Up and Page Down change the month. A native listener
  // keeps the grid a plain group of links (the links are the interactive elements).
  useEffect(() => {
    const el = grid.current;
    if (!el) return;
    const onKeyDown = (e: globalThis.KeyboardEvent) => {
      if (!CALENDAR_KEYS.includes(e.key) || e.altKey || e.ctrlKey || e.metaKey) return;
      const from = (e.target as HTMLElement).closest<HTMLElement>('[data-date]')?.dataset['date'];
      if (!from) return;
      e.preventDefault();
      let next: string | null;
      if (e.key === 'PageUp' || e.key === 'PageDown') {
        const target = addMonths(month, e.key === 'PageUp' ? -1 : 1);
        next = `${target}-${String(Math.min(Number(from.slice(8)), daysOfMonth(target).length)).padStart(2, '0')}`;
      } else {
        next = moveFocus(from, e.key);
      }
      if (!next) return;
      if (next.startsWith(month)) {
        setFocusDay(next);
        el.querySelector<HTMLElement>(`[data-date="${next}"]`)?.focus();
      } else {
        void navigate(`/calendar/${next.slice(0, 7)}`, { state: { focus: next } });
      }
    };
    el.addEventListener('keydown', onKeyDown);
    return () => el.removeEventListener('keydown', onKeyDown);
  }, [month, navigate]);

  const selected = byDate.get(focusDay);
  const isCurrentMonth = today.startsWith(month);
  const desktop = useIsDesktop();

  const heads = (
    <div className="cal-head" aria-hidden="true">
      {WEEKDAYS.map((d) => (
        <span key={d}>{d}</span>
      ))}
    </div>
  );
  const cells = (
    <div className="cal-grid" ref={grid} role="group" aria-label="Days, use the arrow keys to move between them">
      {Array.from({ length: leadingBlanks(month) }, (_, i) => (
        <span key={`lead-${i}`} aria-hidden="true" />
      ))}
      {days.map((d) => {
        const markers = dayMarkers(byDate.get(d));
        const words = activityWords(byDate.get(d));
        const n = Number(d.slice(8));
        const label = [`${n} ${monthName}`, words, d === today ? 'today' : null].filter(Boolean).join(', ');
        return (
          <Link
            key={d}
            to={`/day/${d}`}
            data-date={d}
            className={d > today ? 'cal-day cal-day--future' : 'cal-day'}
            aria-current={d === today ? 'date' : undefined}
            aria-label={label}
            tabIndex={d === focusDay ? 0 : -1}
            onFocus={() => setFocusDay(d)}
          >
            {n}
            <span style={{ display: 'flex', gap: 3, height: desktop ? 8 : 6 }} aria-hidden="true">
              {markers.length === 0 ? <span className="dot" style={{ background: 'none' }} /> : markers.map((m) => <span key={m} className={`dot dot--${m}`} />)}
            </span>
          </Link>
        );
      })}
      {desktop
        ? Array.from({ length: (7 - ((leadingBlanks(month) + days.length) % 7)) % 7 }, (_, i) => <span key={`trail-${i}`} aria-hidden="true" />)
        : null}
    </div>
  );
  const legend = (
    <ul className="legend">
      <li className="legend__item">
        <span className="dot dot--cardio" aria-hidden="true" />
        <span className="legend__label">Cardio (circle)</span>
      </li>
      <li className="legend__item">
        <span className="dot dot--strength" aria-hidden="true" />
        <span className="legend__label">Strength (square)</span>
      </li>
    </ul>
  );

  if (desktop) {
    return (
      <div className="page" style={{ paddingTop: 0 }}>
        <StatsColumns
          asideRight
          side={
            <>
              <StatsSection title={fmtShortDay(focusDay)}>
                <DaySummary day={selected} />
                <LinkButton to={`/day/${focusDay}`}>Open day view</LinkButton>
              </StatsSection>
              <StatsSection title={isCurrentMonth ? 'Month so far' : 'Month totals'}>
                <StatStrip
                  items={[
                    { key: 'activities', label: 'Activities', value: fmtInt(data.totals.activities) },
                    { key: 'strength', label: 'Strength sessions', value: fmtInt(data.totals.strength_sessions) },
                    { key: 'cardio', label: 'Cardio sessions', value: fmtInt(data.totals.cardio_sessions) },
                  ]}
                />
              </StatsSection>
            </>
          }
        >
          <section className="cal-sheet" aria-label={`Calendar for ${fmtMonth(month)}`}>
            {heads}
            {cells}
            {legend}
          </section>
        </StatsColumns>
      </div>
    );
  }

  return (
    <div className="page">
      <div className="grid-panels" style={{ alignItems: 'start' }}>
        <Card padding="sm" aria-label={`Calendar for ${fmtMonth(month)}`} className="cal">
          {heads}
          {cells}
          {legend}
        </Card>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--gap)' }}>
          <Card padding="lg" aria-labelledby="cal-day-title">
            <CardHeader title={fmtShortDay(focusDay)} id="cal-day-title" />
            <DaySummary day={selected} />
            <LinkButton to={`/day/${focusDay}`}>Open day view</LinkButton>
          </Card>
          <Card padding="lg" aria-labelledby="cal-totals-title">
            <CardHeader title={isCurrentMonth ? 'Month so far' : 'Month totals'} id="cal-totals-title" />
            <DataList
              items={[
                { label: 'Activities', value: fmtInt(data.totals.activities) },
                { label: 'Strength sessions', value: fmtInt(data.totals.strength_sessions) },
                { label: 'Cardio sessions', value: fmtInt(data.totals.cardio_sessions) },
              ]}
            />
          </Card>
        </div>
      </div>
    </div>
  );
}

function DaySummary({ day }: { day: CalendarDay | undefined }) {
  return (
    <>
      {day && day.activities.length > 0 ? (
        <ul>
          {day.activities.map((a, i) => {
            const row = activityRow(a);
            return (
              <li key={a.id}>
                <Link to={`/activities/${a.id}`} className={i === 0 ? 'plain-row plain-row--first' : 'plain-row'} style={{ textDecoration: 'none' }}>
                  <span>{row.title}</span>
                  {row.sub ? <span className="plain-row__sub">{row.sub}</span> : null}
                </Link>
              </li>
            );
          })}
        </ul>
      ) : (
        <p className="plain-row__sub">No activity on this day.</p>
      )}
      <DataList
        items={[
          { label: 'Sleep', value: day ? sleepText(day) : null },
          { label: 'Steps', value: day ? fmtInt(day.steps) : null },
          { label: 'Resting heart rate', value: day && day.resting_hr_bpm !== null ? `${fmtInt(day.resting_hr_bpm)} bpm` : null },
          { label: 'Energy', value: day ? energyText(day) : null },
        ]}
      />
    </>
  );
}
