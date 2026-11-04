import { describe, expect, it } from 'vitest';

import { subscriberComponentNames } from './scope';

const components = [
  { id: 'dashboard', name: 'Dashboard', position: 2 },
  { id: 'api', name: 'API', position: 1 },
  { id: 'billing', name: 'Billing', position: 3 },
];

describe('subscriber scope', () => {
  it('names the chosen components in the order of the page', () => {
    expect(
      subscriberComponentNames(
        { componentScope: 'selected', componentIds: ['billing', 'api'] },
        components,
      ),
    ).toEqual(['API', 'Billing']);
  });

  it('names nothing for the whole page, and leaves out a component deleted since', () => {
    expect(subscriberComponentNames({ componentScope: 'all', componentIds: [] }, components)).toEqual([]);
    expect(
      subscriberComponentNames({ componentScope: 'selected', componentIds: ['gone', 'api'] }, components),
    ).toEqual(['API']);
  });
});
