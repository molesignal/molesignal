import { describe, expect, it } from 'vitest';

import { chosenComponentIds, subscriptionAction } from './actions';

describe('subscription links', () => {
  it('reads the actions that carry a token', () => {
    expect(subscriptionAction('#subscription_action=manage&subscription_token=abc')).toEqual({
      kind: 'manage',
      token: 'abc',
    });
    expect(subscriptionAction('#subscription_action=confirm&subscription_token=t.1')).toEqual({
      kind: 'confirm',
      token: 't.1',
    });
  });

  it('reads the return from Add to Slack without a token', () => {
    expect(subscriptionAction('#subscription_action=slack_connected')).toEqual({ kind: 'slack_connected' });
    expect(subscriptionAction('#subscription_action=slack_failed')).toEqual({ kind: 'slack_failed' });
  });

  it('ignores a token action without its token and anything else', () => {
    expect(subscriptionAction('#subscription_action=manage')).toBeNull();
    expect(subscriptionAction('#section=history')).toBeNull();
    expect(subscriptionAction('')).toBeNull();
  });

  it('names no components for the whole page', () => {
    expect(chosenComponentIds('all', ['api'])).toEqual([]);
    expect(chosenComponentIds('selected', ['api'])).toEqual(['api']);
  });
});
