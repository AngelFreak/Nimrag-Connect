import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { EnergyDayChart } from './EnergyDayChart';
import { makeToday } from './testData';

function stressWith(levels: ('rest' | 'low' | 'activity')[]) {
  const today = makeToday();
  return {
    ...today.stress,
    series: levels.map((level, i) => ({
      time: `2026-11-19T0${i}:00:00+00:00`,
      stress: level === 'activity' ? null : level === 'rest' ? 10 : 30,
      level,
    })),
  };
}

describe('EnergyDayChart legend', () => {
  it('names the bars the watch could not measure because of movement, on days that have them', () => {
    const today = makeToday();
    render(<EnergyDayChart energy={today.energy} stress={stressWith(['rest', 'activity', 'low'])} timezone="UTC" />);
    expect(screen.getByText('Too active to measure')).toBeInTheDocument();
  });

  it('leaves that name out on days without such bars', () => {
    const today = makeToday();
    render(<EnergyDayChart energy={today.energy} stress={stressWith(['rest', 'low'])} timezone="UTC" />);
    expect(screen.getByText('Rest')).toBeInTheDocument();
    expect(screen.queryByText('Too active to measure')).not.toBeInTheDocument();
  });
});
