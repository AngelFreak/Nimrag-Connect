import type { CSSProperties } from 'react';
import { StageBar, Unavailable } from '../../components';
import { fmtSeconds } from '../health/model';
import { LEVEL_COLOR, LEVEL_NAME, levelShares, type LevelSeconds } from '../../lib/stress';
import { Panel } from '../health/dayCards';

/** "Time in stress levels": one stacked bar over the day, and under it each level with its time and share. */
export function StressLevelsPanel({ secs }: { secs: LevelSeconds }) {
  const shares = levelShares(secs);
  const summary = shares.map((s) => `${LEVEL_NAME[s.level].toLowerCase()} ${fmtSeconds(s.seconds)}, ${s.percent}%`).join('; ');
  return (
    <Panel title="Time in stress levels" muted>
      {shares.length === 0 ? (
        <p>
          <Unavailable />
        </p>
      ) : (
        <div className="stress-levels" data-testid="stress-levels">
          <StageBar
            height={12}
            label={`Time in stress levels: ${summary}`}
            segments={shares.map((s) => ({ key: s.level, label: LEVEL_NAME[s.level], value: s.seconds, color: LEVEL_COLOR[s.level] }))}
          />
          <ul className="stress-levels__list">
            {shares.map((s) => (
              <li key={s.level}>
                <span className="swatch" aria-hidden="true" style={{ '--swatch': LEVEL_COLOR[s.level] } as CSSProperties} />
                <span className="stress-levels__name">{LEVEL_NAME[s.level]}</span>
                <span>{`${fmtSeconds(s.seconds)} · ${s.percent}%`}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </Panel>
  );
}
