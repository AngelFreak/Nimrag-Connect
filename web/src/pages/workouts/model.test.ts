import { describe, expect, it } from 'vitest';
import type { WorkoutBody } from '../../api/generated';
import {
  clockText,
  DEFAULT_LIMITS,
  draftOf,
  durationText,
  emptyDraft,
  fieldKey,
  fileSteps,
  moved,
  newRepeat,
  newStep,
  parseClock,
  stepCount,
  stepText,
  type TargetKind,
  targetText,
  totalSeconds,
  totalText,
  validate,
  type WorkoutDraft,
} from './model';

const BODY: WorkoutBody = {
  name: '4 x 400 m',
  sport: 'running',
  note: 'Easy first lap',
  nodes: [
    { type: 'step', intensity: 'warmup', duration: { kind: 'time', seconds: 600 }, target: { kind: 'heart_rate_zone', zone: 2 } },
    {
      type: 'repeat',
      count: 4,
      steps: [
        { intensity: 'active', duration: { kind: 'distance', meters: 400 }, target: { kind: 'pace', fast_s_per_km: 270, slow_s_per_km: 300 }, note: 'Fast' },
        { intensity: 'recovery', duration: { kind: 'time', seconds: 90 }, target: { kind: 'none' } },
      ],
    },
    { type: 'step', intensity: 'cooldown', duration: { kind: 'open' }, target: { kind: 'speed', low_kmh: 8, high_kmh: 9.5 } },
  ],
};

function draftWith(over: Partial<WorkoutDraft> = {}): WorkoutDraft {
  return { ...emptyDraft(), name: 'Easy run', ...over };
}

describe('clock text', () => {
  it('reads m:ss, h:mm:ss and a plain number of minutes', () => {
    expect(parseClock('5:00')).toBe(300);
    expect(parseClock(' 0:45 ')).toBe(45);
    expect(parseClock('1:30:05')).toBe(5405);
    expect(parseClock('12')).toBe(720);
  });

  it('refuses anything else', () => {
    for (const text of ['', ':', '5:7', '5:60', '1:2:3', '-3', '5.5', 'abc', '5 min']) expect(parseClock(text), text).toBeNull();
  });

  it('writes seconds the way it reads them', () => {
    expect(clockText(45)).toBe('0:45');
    expect(clockText(300)).toBe('5:00');
    expect(clockText(3600)).toBe('1:00:00');
    expect(clockText(5405)).toBe('1:30:05');
    for (const s of [1, 59, 60, 3599, 3600, 86400]) expect(parseClock(clockText(s))).toBe(s);
  });
});

describe('the draft', () => {
  it('turns a stored workout into form text and back into the same body', () => {
    const draft = draftOf(BODY);
    expect(draft.nodes).toHaveLength(3);
    const first = draft.nodes[0];
    expect(first?.type === 'step' && first.step.duration).toBe('10:00');
    const result = validate(draft);
    expect(result.errors).toEqual({ fields: {} });
    expect(result.body).toEqual(BODY);
  });

  it('keeps keys out of the saved body and gives every step its own', () => {
    const draft = draftOf(BODY);
    const keys = draft.nodes.flatMap((n) => (n.type === 'step' ? [n.key, n.step.key] : [n.key, ...n.steps.map((s) => s.key)]));
    expect(new Set(keys).size).toBe(keys.length);
    expect(JSON.stringify(validate(draft).body)).not.toContain('"key"');
  });

  it('drops an empty note and trims the name', () => {
    const result = validate(draftWith({ name: '  Easy run  ', note: '   ' }));
    expect(result.body?.name).toBe('Easy run');
    expect(result.body && 'note' in result.body).toBe(false);
  });

  it('starts with one warm-up step that is valid once it has a name', () => {
    expect(validate(emptyDraft()).errors.name).toBe('Give the workout a name.');
    const ok = validate(draftWith());
    expect(ok.body?.nodes).toEqual([{ type: 'step', intensity: 'warmup', duration: { kind: 'time', seconds: 600 }, target: { kind: 'none' } }]);
  });

  it('moves items and leaves the rest alone', () => {
    expect(moved(['a', 'b', 'c'], 0, 1)).toEqual(['b', 'a', 'c']);
    expect(moved(['a', 'b', 'c'], 2, 1)).toEqual(['a', 'c', 'b']);
    expect(moved(['a', 'b'], 0, -1)).toEqual(['a', 'b']);
    expect(moved(['a', 'b'], 1, 2)).toEqual(['a', 'b']);
  });
});

describe('checking the draft', () => {
  it('needs a name, in at most the limit of bytes', () => {
    expect(validate(draftWith({ name: '   ' })).errors.name).toBe('Give the workout a name.');
    expect(validate(draftWith({ name: 'a'.repeat(63) })).body).not.toBeNull();
    expect(validate(draftWith({ name: 'a'.repeat(64) })).errors.name).toMatch(/at most 63 bytes/);
    // 32 two-byte letters are 64 bytes.
    expect(validate(draftWith({ name: 'å'.repeat(32) })).errors.name).toMatch(/at most 63 bytes/);
    expect(validate(draftWith({ name: 'å'.repeat(31) })).body).not.toBeNull();
  });

  it('keeps the note under its limit', () => {
    expect(validate(draftWith({ note: 'n'.repeat(254) })).body).not.toBeNull();
    expect(validate(draftWith({ note: 'n'.repeat(255) })).errors.note).toMatch(/at most 254 bytes/);
  });

  it('needs at least one step and at most the limit', () => {
    expect(validate(draftWith({ nodes: [] })).errors.form).toBe('The workout has no steps. Add one.');
    const many = Array.from({ length: 101 }, () => ({ type: 'step' as const, key: `s${Math.random()}`, step: newStep() }));
    expect(validate(draftWith({ nodes: many })).errors.form).toMatch(/at most 100 steps/);
    expect(validate(draftWith({ nodes: many.slice(0, 100) })).body).not.toBeNull();
  });

  it('counts a repeat block as one step more than the steps in it, like the file does', () => {
    const block = newRepeat();
    expect(block.type === 'repeat' && block.steps).toHaveLength(2);
    expect(fileSteps([block])).toBe(3);
    expect(stepCount([block])).toBe(2);
    const nodes = [block, ...Array.from({ length: 97 }, () => ({ type: 'step' as const, key: `s${Math.random()}`, step: newStep() }))];
    expect(fileSteps(nodes)).toBe(100);
    expect(validate(draftWith({ nodes })).body).not.toBeNull();
    expect(validate(draftWith({ nodes: [...nodes, nodes[1]!] })).errors.form).toMatch(/at most 100 steps/);
  });

  it('checks every kind of end', () => {
    const check = (durationKind: 'time' | 'distance' | 'heart_rate_below' | 'heart_rate_above' | 'open', duration: string) => {
      const step = newStep({ durationKind, duration });
      const result = validate(draftWith({ nodes: [{ type: 'step', key: 'n', step }] }));
      return { body: result.body, error: result.errors.fields[fieldKey(step.key, 'duration')] };
    };
    expect(check('time', '0:00').error).toMatch(/1 second to 24 hours/);
    expect(check('time', '24:00:01').error).toMatch(/1 second to 24 hours/);
    expect(check('time', '24:00:00').body).not.toBeNull();
    expect(check('time', 'soon').error).toMatch(/like 5:00/);
    expect(check('distance', '0').error).toMatch(/1 metre to 500 km/);
    expect(check('distance', '500001').error).toMatch(/1 metre to 500 km/);
    expect(check('distance', '1.5').error).toMatch(/whole metres/);
    expect(check('distance', '500000').body).not.toBeNull();
    expect(check('heart_rate_below', '29').error).toMatch(/30 to 240 bpm/);
    expect(check('heart_rate_above', '241').error).toMatch(/30 to 240 bpm/);
    expect(check('heart_rate_above', '240').body?.nodes[0]).toMatchObject({ duration: { kind: 'heart_rate_above', bpm: 240 } });
    expect(check('open', '').body?.nodes[0]).toMatchObject({ duration: { kind: 'open' } });
  });

  it('checks every kind of target, low not above high', () => {
    const check = (targetKind: TargetKind, a: string, b = '') => {
      const step = newStep({ targetKind, targetA: a, targetB: b });
      const result = validate(draftWith({ nodes: [{ type: 'step', key: 'n', step }] }));
      return { target: (result.body?.nodes[0] as { target?: unknown } | undefined)?.target, error: result.errors.fields[fieldKey(step.key, 'target')] };
    };
    expect(check('heart_rate_zone', '3').target).toEqual({ kind: 'heart_rate_zone', zone: 3 });
    expect(check('heart_rate_zone', '').error).toMatch(/1 to 5/);
    expect(check('heart_rate_zone', '6').error).toMatch(/1 to 5/);
    expect(check('heart_rate_range', '120', '140').target).toEqual({ kind: 'heart_rate_range', low_bpm: 120, high_bpm: 140 });
    expect(check('heart_rate_range', '140', '120').error).toMatch(/low not above high/);
    expect(check('heart_rate_range', '20', '120').error).toMatch(/30 to 240/);
    expect(check('pace', '5:30', '6:15').target).toEqual({ kind: 'pace', fast_s_per_km: 330, slow_s_per_km: 375 });
    expect(check('pace', '6:15', '5:30').error).toMatch(/fast not slower than slow/);
    expect(check('pace', '0:30', '5:00').error).toMatch(/1:00 to 60:00/);
    expect(check('pace', '5:30', '').error).toMatch(/both paces/);
    expect(check('speed', '9', '11,5').target).toEqual({ kind: 'speed', low_kmh: 9, high_kmh: 11.5 });
    expect(check('speed', '0.4', '9').error).toMatch(/0.5 to 100 km\/h/);
    expect(check('speed', '12', '9').error).toMatch(/low not above high/);
    expect(check('cadence', '160', '175').target).toEqual({ kind: 'cadence', low: 160, high: 175 });
    expect(check('cadence', '0', '175').error).toMatch(/1 to 300/);
    expect(check('cadence', '170', '301').error).toMatch(/1 to 300/);
    expect(check('none', '').target).toEqual({ kind: 'none' });
  });

  it('checks the repeat count and keeps blocks to their size', () => {
    const block = newRepeat();
    if (block.type !== 'repeat') throw new Error('a repeat');
    const run = (count: string, steps = block.steps) => validate(draftWith({ nodes: [{ ...block, count, steps }] }));
    expect(run('4').body?.nodes[0]).toMatchObject({ type: 'repeat', count: 4 });
    expect(run('1').errors.fields[fieldKey(block.key, 'count')]).toMatch(/2 to 99 times/);
    expect(run('100').errors.fields[fieldKey(block.key, 'count')]).toMatch(/2 to 99 times/);
    expect(run('x').body).toBeNull();
    expect(run('99').body).not.toBeNull();
    expect(run('4', []).errors.fields[fieldKey(block.key, 'steps')]).toMatch(/1 to 20 steps/);
    const twentyOne = Array.from({ length: 21 }, () => newStep());
    expect(run('4', twentyOne).errors.fields[fieldKey(block.key, 'steps')]).toMatch(/1 to 20 steps/);
  });

  it('points at the step that is wrong, inside a block too', () => {
    const block = newRepeat();
    if (block.type !== 'repeat') throw new Error('a repeat');
    const bad = block.steps[1]!;
    bad.duration = 'nope';
    const result = validate(draftWith({ nodes: [block] }));
    expect(result.body).toBeNull();
    expect(Object.keys(result.errors.fields)).toEqual([fieldKey(bad.key, 'duration')]);
  });

  it('uses the limits it is given', () => {
    const tight = { ...DEFAULT_LIMITS, name_max_bytes: 5 };
    expect(validate(draftWith({ name: 'abcdef' }), tight).errors.name).toMatch(/at most 5 bytes/);
  });
});

describe('words for a stored workout', () => {
  it('describes ends and targets', () => {
    expect(durationText({ kind: 'time', seconds: 90 })).toBe('1:30');
    expect(durationText({ kind: 'distance', meters: 400 })).toBe('400 m');
    expect(durationText({ kind: 'distance', meters: 5000 })).toBe('5 km');
    expect(durationText({ kind: 'distance', meters: 1250 })).toBe('1250 m');
    expect(durationText({ kind: 'open' })).toBe('until lap');
    expect(durationText({ kind: 'heart_rate_below', bpm: 120 })).toBe('until HR below 120');
    expect(targetText({ kind: 'none' })).toBeNull();
    expect(targetText({ kind: 'heart_rate_zone', zone: 2 })).toBe('zone 2');
    expect(targetText({ kind: 'pace', fast_s_per_km: 270, slow_s_per_km: 300 })).toBe('4:30 to 5:00 /km');
    expect(stepText({ intensity: 'warmup', duration: { kind: 'time', seconds: 600 }, target: { kind: 'heart_rate_zone', zone: 2 } })).toBe('Warm-up, 10:00, zone 2');
  });

  it('adds up the time only when every step has one', () => {
    expect(totalSeconds([{ type: 'step', intensity: 'warmup', duration: { kind: 'time', seconds: 600 }, target: { kind: 'none' } }])).toBe(600);
    const repeat = BODY.nodes[1]!;
    expect(totalSeconds([repeat])).toBeNull();
    expect(
      totalSeconds([
        { type: 'repeat', count: 3, steps: [{ intensity: 'active', duration: { kind: 'time', seconds: 60 }, target: { kind: 'none' } }, { intensity: 'rest', duration: { kind: 'time', seconds: 30 }, target: { kind: 'none' } }] },
      ]),
    ).toBe(270);
    expect(totalText(null)).toBeNull();
    expect(totalText(30)).toBe('1 min');
    expect(totalText(2700)).toBe('45 min');
    expect(totalText(3900)).toBe('1 h 05 min');
  });
});
