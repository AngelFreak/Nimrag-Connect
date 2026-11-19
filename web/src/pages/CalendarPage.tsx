import { useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { Card, CardHeader, DataList, Legend, TopBar } from '../components';
import { addMonths, browserToday, fmtMonth, fmtShortDay, isIsoMonth } from '../lib/dates';
import { usePageTitle } from '../lib/hooks';

// STUB (stage 3l wires it to GET /api/calendar/{yyyy-mm}): static canvas markers, shown for
// November 2026 only. `/calendar/:month?` without a month means the current month.

type Kind = 'cardio' | 'strength';
const MARKS: Record<string, { kind: Kind; label: string }> = {
  '2026-11-03': { kind: 'strength', label: 'strength' },
  '2026-11-05': { kind: 'cardio', label: 'run' },
  '2026-11-08': { kind: 'strength', label: 'strength' },
  '2026-11-10': { kind: 'strength', label: 'strength' },
  '2026-11-11': { kind: 'cardio', label: 'run' },
  '2026-11-13': { kind: 'cardio', label: 'walk' },
  '2026-11-15': { kind: 'strength', label: 'strength' },
  '2026-11-16': { kind: 'cardio', label: 'run' },
  '2026-11-18': { kind: 'strength', label: 'strength' },
};
const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

export function CalendarPage() {
  const { month: param } = useParams();
  const today = browserToday();
  const month = isIsoMonth(param) ? param : today.slice(0, 7);
  const [picked, setSelected] = useState('2026-11-18');
  const selected = picked.startsWith(month) ? picked : `${month}-01`;
  const title = fmtMonth(month);
  usePageTitle(title);

  const first = new Date(`${month}-01T00:00:00Z`);
  const lead = (first.getUTCDay() + 6) % 7; // Monday first
  const count = new Date(Date.UTC(first.getUTCFullYear(), first.getUTCMonth() + 1, 0)).getUTCDate();
  const days = Array.from({ length: count }, (_, i) => `${month}-${String(i + 1).padStart(2, '0')}`);
  const mark = MARKS[selected];

  return (
    <>
      <TopBar
        variant="pager"
        title={title}
        subtitle="Calendar · sample data"
        prevLabel="Previous month"
        nextLabel="Next month"
        prev={{ to: `/calendar/${addMonths(month, -1)}` }}
        next={{ to: `/calendar/${addMonths(month, 1)}` }}
      />
      <div className="page">
        <Card padding="sm" aria-label={`Calendar for ${title}`} className="cal">
          <div className="cal-head" aria-hidden="true">
            {WEEKDAYS.map((d) => (
              <span key={d}>{d}</span>
            ))}
          </div>
          <div className="cal-grid">
            {Array.from({ length: lead }, (_, i) => (
              <span key={`lead-${i}`} />
            ))}
            {days.map((d) => {
              const m = MARKS[d];
              const n = Number(d.slice(8));
              const label = `${n} ${fmtMonth(month).split(' ')[0]}${m ? `, ${m.label}` : ''}${d === today ? ', today' : ''}${d === selected ? ', selected' : ''}`;
              return (
                <button
                  key={d}
                  type="button"
                  className={d > today ? 'cal-day cal-day--future' : 'cal-day'}
                  aria-pressed={d === selected}
                  aria-current={d === today ? 'date' : undefined}
                  aria-label={label}
                  onClick={() => setSelected(d)}
                >
                  {n}
                  {m ? <span className={`dot dot--${m.kind}`} /> : <span className="dot" />}
                </button>
              );
            })}
          </div>
          <Legend
            items={[
              { key: 'cardio', label: 'Cardio (circle)', color: 'var(--link)' },
              { key: 'strength', label: 'Strength (square)', color: 'var(--warn)' },
            ]}
          />
        </Card>
        <Card padding="lg">
          <CardHeader title={fmtShortDay(selected)} />
          {mark ? (
            <Link to="/activities" className="plain-row plain-row--first" style={{ textDecoration: 'none' }}>
              <span>{mark.label === 'strength' ? 'Strength · 18:05' : 'Run · 17:40'}</span>
              <span className="plain-row__sub">{mark.label === 'strength' ? '42 min · 18 sets' : '38 min · 6.4 km'}</span>
            </Link>
          ) : (
            <p className="plain-row__sub">No activity on this day.</p>
          )}
          <DataList
            items={[
              { label: 'Sleep', value: '7h 12m · score 81' },
              { label: 'Steps', value: '8,360' },
              { label: 'Resting heart rate', value: '51 bpm' },
              { label: 'Energy', value: 'High 94 · low 38' },
            ]}
          />
          <Link to={`/day/${selected}`}>Open day view</Link>
        </Card>
        <Card padding="lg">
          <CardHeader title="Month so far" />
          <DataList
            items={[
              { label: 'Activities', value: '9' },
              { label: 'Strength sessions', value: '5' },
              { label: 'Cardio sessions', value: '4' },
            ]}
          />
        </Card>
      </div>
    </>
  );
}
