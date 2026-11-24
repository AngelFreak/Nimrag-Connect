import { useQuery } from '@tanstack/react-query';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { api } from '../api/client';
import type { MovementResponse } from '../api/generated';
import {
  BarSeries,
  Card,
  CardHeader,
  DataList,
  ErrorState,
  Legend,
  PageHeader,
  ProgressBar,
  SampleDataChip,
  StageBar,
  StatStrip,
  TopBar,
  Unavailable,
  ValueOrUnavailable,
} from '../components';
import { addDays, browserToday, fmtMonthDay, fmtShortDay, isIsoDay } from '../lib/dates';
import { fmtInt } from '../lib/format';
import { useIsDesktop, usePageTitle } from '../lib/hooks';
import { DayToolbar, ScoreHero, StatsColumns, StatsSection } from './health/desktop';
import { PageError, PageSkeleton, useOwnerToday } from './body/common';
import { fmtKcal, fmtKm, intensityParts, weekStepsSummary, weekdayShort, wholePercent } from './body/movement';

// Stage 3i: GET /api/movement/{date}. The date is part of the URL, /movement/2026-11-18;
// without one it is today.

export function MovementPage() {
  const { date: requested } = useParams();
  const invalid = requested !== undefined && !isIsoDay(requested);
  const ownerToday = useOwnerToday();
  const date = requested !== undefined ? (invalid ? undefined : requested) : ownerToday;
  usePageTitle('Movement');

  const query = useQuery({
    queryKey: ['movement', date],
    queryFn: ({ signal }) => api.get<MovementResponse>(`/api/movement/${date}`, { signal }),
    enabled: date !== undefined,
  });
  const data = query.data;

  const canGoNext = date !== undefined && date < (ownerToday ?? browserToday());
  const subtitle = date ? `${fmtShortDay(date)}${data ? ` · week ${data.week_number}` : ''}` : 'Steps, intensity and calories';

  const desktop = useIsDesktop();
  const navigate = useNavigate();
  return (
    <>
      {desktop ? (
        <PageHeader title="Movement" actions={<SampleDataChip sample={data?.sample} />}>
          <DayToolbar
            label={date ? fmtMonthDay(date) : 'Today'}
            prevLabel="Previous day"
            nextLabel="Next day"
            onPrev={() => date && void navigate(`/movement/${addDays(date, -1)}`)}
            onNext={() => date && void navigate(`/movement/${addDays(date, 1)}`)}
            atEnd={!canGoNext}
            end={
              <>
                {data ? <span className="stats-note">Week {data.week_number}</span> : null}
                <Link className="day-toolbar__link" to="/metrics/steps?range=7d">
                  Steps over time
                </Link>
              </>
            }
          />
        </PageHeader>
      ) : (
        <TopBar
          variant="back"
          backTo="/"
          title="Movement"
          subtitle={subtitle}
          aside={<SampleDataChip sample={data?.sample} />}
          {...(date
            ? {
                prev: { to: `/movement/${addDays(date, -1)}` },
                next: canGoNext ? { to: `/movement/${addDays(date, 1)}` } : null,
                prevLabel: 'Previous day',
                nextLabel: 'Next day',
              }
            : {})}
        />
      )}
      {invalid ? (
        <div className="page">
          <ErrorState
            title="That is not a date"
            action={
              <Link className="btn btn--primary" to="/movement">
                Go to today
              </Link>
            }
          >
            Use the form YYYY-MM-DD, for example 2026-11-18.
          </ErrorState>
        </div>
      ) : query.isError ? (
        <PageError title="Movement could not be loaded" error={query.error} onRetry={() => void query.refetch()} />
      ) : !data ? (
        <PageSkeleton cards={4} />
      ) : (
        <MovementBody data={data} />
      )}
    </>
  );
}

export function MovementBody({ data }: { data: MovementResponse }) {
  const desktop = useIsDesktop();
  const goal = data.step_goal;
  const stepsText = fmtInt(data.steps);
  const percent = wholePercent(data.percent_of_goal);
  const km = fmtKm(data.distance_m);
  const week = data.week_steps;
  const weekValues = week.map((d) => d.value);
  const weekTop = Math.max(goal * 1.25, ...weekValues.filter((v): v is number => v !== null));
  const avg = fmtInt(data.week_average_steps);
  const { total, moderate, vigorous } = intensityParts(data);
  const intensityGoal = data.intensity.goal_min;
  const scale = Math.max(intensityGoal, total ?? 0);

  if (desktop) {
    const stepsLabel = stepsText === null ? 'Steps today unavailable' : `${stepsText} of ${fmtInt(goal)} steps${percent === null ? '' : `, ${percent} percent`}`;
    return (
      <div className="page" style={{ paddingTop: 0 }}>
        <StatsColumns
          side={
            <>
              <StatsSection title="Steps" aside={`Goal ${fmtInt(goal)}`}>
                <ScoreHero value={stepsText ?? <Unavailable />} of={percent === null ? undefined : `${percent}% of the goal`} tone="tone-steps" />
                <ProgressBar value={data.steps} max={goal} height={8} label={stepsLabel} />
              </StatsSection>
              <StatStrip
                items={[
                  { key: 'distance', label: 'Distance', value: km },
                  { key: 'active', label: 'Active calories', value: fmtKcal(data.calories.active_kcal) },
                  { key: 'resting', label: 'Resting calories', value: fmtKcal(data.calories.resting_kcal) },
                  { key: 'total', label: 'Total calories', value: fmtKcal(data.calories.total_kcal) },
                ]}
              />
            </>
          }
        >
          <StatsSection title="Steps this week" aside={avg === null ? undefined : `Avg ${avg} a day`}>
            <BarSeries
              values={weekValues}
              labels={week.map((d) => weekdayShort(d.date))}
              goal={goal}
              max={weekTop}
              color="var(--steps)"
              belowGoalColor="var(--steps-dim)"
              height={200}
              label={weekStepsSummary(week, goal)}
            />
            <Legend
              items={[
                { key: 'met', label: 'Goal reached', color: 'var(--steps)' },
                { key: 'below', label: 'Below goal', color: 'var(--steps-dim)' },
                { key: 'goal', label: 'Goal', color: 'var(--text-2)', shape: 'dash' },
              ]}
            />
            <p className="stats-note">Days after this one have no data yet.</p>
          </StatsSection>

          <StatsSection title="Intensity Minutes" aside={`Weekly goal ${fmtInt(intensityGoal)}`}>
            <span className="big-value">
              <ValueOrUnavailable value={total === null ? null : `${fmtInt(total)} min`} />
            </span>
            {total === null ? null : (
              <>
                <StageBar
                  total={scale}
                  label={`${fmtInt(total)} of ${fmtInt(intensityGoal)} intensity minutes: ${fmtInt(moderate) ?? 'no'} moderate and ${fmtInt((vigorous ?? 0) * 2)} from vigorous activity, counted double`}
                  segments={[
                    { key: 'mod', label: 'Moderate', value: moderate, color: 'var(--zone-1)' },
                    { key: 'vig', label: 'Vigorous', value: vigorous === null ? null : vigorous * 2, color: 'var(--warn)' },
                  ]}
                />
                <Legend
                  items={[
                    { key: 'mod', label: 'Moderate', color: 'var(--zone-1)', value: fmtInt(moderate) },
                    { key: 'vig', label: 'Vigorous, counted double', color: 'var(--warn)', value: fmtInt(vigorous) },
                  ]}
                />
              </>
            )}
          </StatsSection>
        </StatsColumns>
      </div>
    );
  }

  return (
    <div className="page">
      <div className="grid-panels">
        <Card padding="lg">
          <CardHeader title="Steps today" aside={`Goal ${fmtInt(goal)}`} />
          <span className="stat__value" style={{ fontSize: 34 }}>
            <ValueOrUnavailable value={stepsText} />
          </span>
          <ProgressBar
            value={data.steps}
            max={goal}
            height={8}
            label={stepsText === null ? 'Steps today unavailable' : `${stepsText} of ${fmtInt(goal)} steps${percent === null ? '' : `, ${percent} percent`}`}
          />
          <span className="stat__sub">{percent === null ? <Unavailable /> : `${percent}% of the goal`}</span>
          <DataList hideMissing items={[{ label: 'Distance', value: km }]} />
        </Card>

        <Card padding="lg">
          <CardHeader title="Steps this week" aside={avg === null ? <Unavailable /> : `Avg ${avg} a day`} />
          <BarSeries
            values={weekValues}
            labels={week.map((d) => weekdayShort(d.date))}
            goal={goal}
            max={weekTop}
            color="var(--steps)"
            belowGoalColor="var(--steps-dim)"
            height={100}
            label={weekStepsSummary(week, goal)}
          />
          <p className="stat__sub">Days after this one have no data yet. Bars below the goal are drawn dimmer.</p>
        </Card>

        <Card padding="lg">
          <CardHeader title="Intensity minutes" aside={`Weekly goal ${fmtInt(intensityGoal)}`} />
          <span className="stat__value" style={{ fontSize: 28 }}>
            <ValueOrUnavailable value={total === null ? null : `${fmtInt(total)} min`} />
          </span>
          {total === null ? null : (
            <>
              <StageBar
                total={scale}
                label={`${fmtInt(total)} of ${fmtInt(intensityGoal)} intensity minutes: ${fmtInt(moderate) ?? 'no'} moderate and ${fmtInt((vigorous ?? 0) * 2)} from vigorous activity, counted double`}
                segments={[
                  { key: 'mod', label: 'Moderate', value: moderate, color: 'var(--zone-1)' },
                  { key: 'vig', label: 'Vigorous', value: vigorous === null ? null : vigorous * 2, color: 'var(--warn)' },
                ]}
              />
              <Legend
                items={[
                  { key: 'mod', label: 'Moderate', color: 'var(--zone-1)', value: fmtInt(moderate) },
                  { key: 'vig', label: 'Vigorous, counted double', color: 'var(--warn)', value: fmtInt(vigorous) },
                ]}
              />
            </>
          )}
        </Card>

        <Card padding="lg">
          <CardHeader title="Calories today" />
          <DataList
            items={[
              { label: 'Active', value: fmtKcal(data.calories.active_kcal) },
              { label: 'Resting', value: fmtKcal(data.calories.resting_kcal) },
              { label: 'Total', value: fmtKcal(data.calories.total_kcal), strong: true },
            ]}
          />
        </Card>
      </div>
    </div>
  );
}
