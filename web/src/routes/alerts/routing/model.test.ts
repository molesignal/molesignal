import { describe, expect, it } from 'vitest';

import { conditionText, dropped, labelsFromText, moved, targetExists, valuesFromText } from './model';

describe('alert routing model', () => {
  it('reads a condition the way the list shows it', () => {
    expect(conditionText({ key: 'env', operator: 'equals', values: ['prod'] })).toBe('env = prod');
    expect(conditionText({ key: 'env', operator: 'not_equals', values: ['dev'] })).toBe('env ≠ dev');
    expect(conditionText({ key: 'team', operator: 'in', values: ['a', 'b'] })).toBe('team ∈ a, b');
    expect(conditionText({ key: 'db.system', operator: 'matches', values: ['postgres.*'] })).toBe(
      'db.system ~ postgres.*',
    );
  });

  it('takes a list for "in" and one value for the other comparisons', () => {
    expect(valuesFromText('in', 'a, b\nc,,')).toEqual(['a', 'b', 'c']);
    expect(valuesFromText('equals', '  prod, eu ')).toEqual(['prod, eu']);
    expect(valuesFromText('matches', '   ')).toEqual([]);
  });

  it('reads labels one per line', () => {
    expect(labelsFromText('service.name=checkout\n team = payments \nbroken\n=nokey\nurl=a=b')).toEqual({
      'service.name': 'checkout',
      team: 'payments',
      url: 'a=b',
    });
  });

  it('moves a rule within the order', () => {
    expect(moved(['a', 'b', 'c'], 2, 0)).toEqual(['c', 'a', 'b']);
    expect(moved(['a', 'b', 'c'], 0, 1)).toEqual(['b', 'a', 'c']);
    expect(moved(['a', 'b', 'c'], 1, 9)).toEqual(['a', 'c', 'b']);
  });

  it('puts a dropped rule beside the one it was dropped on', () => {
    expect(dropped(['a', 'b', 'c'], 'c', 'a', 'before')).toEqual(['c', 'a', 'b']);
    expect(dropped(['a', 'b', 'c'], 'a', 'b', 'after')).toEqual(['b', 'a', 'c']);
    expect(dropped(['a', 'b', 'c'], 'b', 'b', 'after')).toEqual(['a', 'b', 'c']);
    expect(dropped(['a', 'b', 'c'], 'x', 'b', 'after')).toEqual(['a', 'b', 'c']);
  });

  it('knows a rule whose target is gone', () => {
    const policies = new Set(['pol-1']);
    const teams = new Set(['payments']);

    expect(targetExists({ kind: 'policy', policyId: 'pol-1' }, policies, teams)).toBe(true);
    expect(targetExists({ kind: 'team', teamId: 'search' }, policies, teams)).toBe(false);
  });
});
