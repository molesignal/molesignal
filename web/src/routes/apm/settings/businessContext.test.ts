import { describe, expect, it } from 'vitest';

import { blankBusinessKey, businessKeyIssues, contextOf, draftOf } from './businessContext';

describe('business context form', () => {
  it('saves what was typed, blank fields naming nothing and important values once each', () => {
    const context = contextOf([
      { key: ' tenant ', label: ' Tenant ', span: 'tenant.id', log: ' ', rum: 'context.tenant', important: 'acme, globex\nacme,' },
    ]);

    expect(context).toEqual({
      keys: [{ key: 'tenant', label: 'Tenant', span: 'tenant.id', rum: 'context.tenant', important: ['acme', 'globex'] }],
    });
    expect(draftOf(context)[0]).toEqual({
      key: 'tenant',
      label: 'Tenant',
      span: 'tenant.id',
      log: '',
      rum: 'context.tenant',
      important: 'acme, globex',
    });
  });

  it('says a key named badly, twice, or carried nowhere', () => {
    const issues = businessKeyIssues([
      { ...blankBusinessKey(), key: 'Tenant', span: 'x' },
      { ...blankBusinessKey(), key: 'plan', span: 'plan' },
      { ...blankBusinessKey(), key: 'plan', log: 'plan' },
      { ...blankBusinessKey(), key: 'tier' },
    ]);

    expect([...issues.entries()]).toEqual([
      [0, 'key'],
      [2, 'twice'],
      [3, 'nowhere'],
    ]);
  });
});
