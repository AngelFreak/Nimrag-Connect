import type { Today } from '../../api/generated';
import { Card, Ring, StatusWord, Unavailable } from '../../components';
import { fmtTime, energyHeadline, signed } from './format';
import { energyKey } from '../../lib/status';
import { isMissing } from '../../lib/format';
import { EnergyDayChart } from './EnergyDayChart';

/** Energy on a phone: the one wide card at the top of Today, with the ring, the flows and the day's line. */
export function EnergyCard({ today }: { today: Today }) {
  const e = today.energy;
  const headline = energyHeadline(e.now);
  const woke = fmtTime(e.wake_time, today.timezone);
  return (
    <section className="today-section" aria-label="Energy" data-card="energy">
      <Card as="div" padding="lg">
        <div className="focus-energy">
          <Ring value={e.now} label="Energy level" color="var(--energy)" />
          <div className="focus-energy__text">
            <span className="stat__label">Energy</span>
            <span className="focus-energy__head">{headline ? <StatusWord type="energy" value={energyKey(e.now)} size="lg">{headline}</StatusWord> : <Unavailable />}</span>
            <div className="focus-energy__flows">
              <span className="tone-energy">{isMissing(e.charged) ? <>Charged <Unavailable /></> : `${signed(e.charged)} charged`}</span>
              <span className="tone-warn">{isMissing(e.drained) ? <>Drained <Unavailable /></> : `${signed(-e.drained)} drained`}</span>
              {woke ? (
                <span className="focus-energy__woke">
                  <i className="day-chart__wake" aria-hidden="true" />
                  Woke {woke}
                </span>
              ) : null}
            </div>
          </div>
        </div>
        <EnergyDayChart energy={e} stress={today.stress} timezone={today.timezone} />
      </Card>
    </section>
  );
}
