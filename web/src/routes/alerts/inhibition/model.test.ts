import { describe, expect, it } from 'vitest';

import { cleanInput, equalFromText, newRuleInput, ruleProblem, ruleStanding } from './model';

describe('inhibit rule form', () => {
  it('starts with the common rule: the worse holds back the lesser on one service', () => {
    const rule = newRuleInput();

    expect(rule.equal).toEqual(['service']);
    expect(rule.moreSevereOnly).toBe(true);
    expect(ruleProblem({ ...rule, name: 'Same service' })).toBeNull();
  });

  it('reads shared labels from a comma separated line, each once', () => {
    expect(equalFromText(' service, env ,,service')).toEqual(['service', 'env']);
    expect(equalFromText('  ')).toEqual([]);
  });

  it('drops conditions that have no label yet', () => {
    const rule = newRuleInput();
    rule.name = '  Named ';
    rule.source.matchers = [
      { label: '', op: 'eq', value: '' },
      { label: 'service', op: 'eq', value: 'db' },
    ];

    const clean = cleanInput(rule);

    expect(clean.name).toBe('Named');
    expect(clean.source.matchers).toEqual([{ label: 'service', op: 'eq', value: 'db' }]);
  });

  it('refuses what the server refuses', () => {
    const rule = { ...newRuleInput(), name: 'x' };

    expect(ruleProblem({ ...rule, name: ' ' })).toBe('name');
    expect(ruleProblem({ ...rule, equal: [] })).toBe('no_scope');
    expect(ruleProblem({ ...rule, waitSecs: 601 })).toBe('wait');
    expect(ruleProblem({ ...rule, lingerSecs: -1 })).toBe('linger');
    expect(
      ruleProblem({ ...rule, equal: [], source: { matchers: [{ label: 'service', op: 'eq', value: 'db' }] } }),
    ).toBeNull();
  });

  it('says whether a rule is holding anything back', () => {
    expect(ruleStanding({ enabled: true, inhibiting: 2 })).toBe('holding');
    expect(ruleStanding({ enabled: true, inhibiting: 0 })).toBe('on');
    expect(ruleStanding({ enabled: false, inhibiting: 0 })).toBe('off');
  });
});
