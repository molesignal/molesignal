import { describe, expect, it } from 'vitest';

import { feedbackKindKey, lookupIds } from './useFeedback';

describe('feedback lookups', () => {
  it('ask for each target once, in one order', () => {
    expect(lookupIds(['m2', 'm1', '', 'm2'])).toEqual(['m1', 'm2']);
  });

  it('keep every kind under its own cache key', () => {
    expect(feedbackKindKey('chat_message')).toEqual(['agent', 'feedback', 'chat_message']);
    expect(feedbackKindKey('incident_rca')).toEqual(['agent', 'feedback', 'incident_rca']);
  });
});
