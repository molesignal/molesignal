import { describe, expect, it } from 'vitest';

import { orgInitial } from './OrgSwitcher';

describe('orgInitial', () => {
  it('takes the first letter or figure, upper-cased', () => {
    expect(orgInitial('acme-staging')).toBe('A');
    expect(orgInitial('_sys')).toBe('S');
    expect(orgInitial('研发中心')).toBe('研');
  });

  it('falls back to a dot when the name has no letter', () => {
    expect(orgInitial('--')).toBe('·');
  });
});
