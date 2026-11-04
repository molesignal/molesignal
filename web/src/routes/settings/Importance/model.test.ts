import { describe, expect, it } from 'vitest';

import { DEFAULT_LEVELS } from '@/api/importance';

import { giveSeverity, levelsProblem, moveUrgency, newUrgency, withPriorityCount } from './model';

describe('importance levels model', () => {
  it('keeps the priority keys P1 to Pn and their names as the count changes', () => {
    const named = { ...DEFAULT_LEVELS, priorities: DEFAULT_LEVELS.priorities.map((level, index) => ({ ...level, name: index === 0 ? 'Now' : '' })) };

    expect(withPriorityCount(named, 3).priorities.map((level) => level.priority)).toEqual(['P1', 'P2', 'P3']);
    expect(withPriorityCount(named, 3).priorities[0]?.name).toBe('Now');
    expect(withPriorityCount(named, 12).priorities).toHaveLength(9);
    expect(withPriorityCount(named, 0).priorities).toHaveLength(1);
  });

  it('gives a severity to one urgency and refuses levels the server would refuse', () => {
    const three = { ...DEFAULT_LEVELS, urgencies: [DEFAULT_LEVELS.urgencies[0]!, newUrgency('medium'), DEFAULT_LEVELS.urgencies[1]!] };

    expect(levelsProblem(three)).toBeNull();

    const moved = giveSeverity(three, 'medium', 'error');
    expect(moved.urgencies.map((level) => [level.key, level.severities])).toEqual([
      ['high', ['critical']],
      ['medium', ['error']],
      ['low', ['warning', 'info']],
    ]);
    expect(levelsProblem(moved)).toBeNull();
    expect(levelsProblem({ ...moved, urgencies: moved.urgencies.slice(0, 2) })).toBe('built_in');
    expect(levelsProblem({ ...moved, urgencies: [...moved.urgencies, newUrgency('Night')] })).toBe('key');
    expect(levelsProblem({ ...moved, urgencies: [...moved.urgencies, newUrgency('medium')] })).toBe('key_twice');
    expect(moveUrgency(moved, 1, -1).urgencies.map((level) => level.key)).toEqual(['medium', 'high', 'low']);
    expect(moveUrgency(moved, 0, -1)).toBe(moved);
  });
});
