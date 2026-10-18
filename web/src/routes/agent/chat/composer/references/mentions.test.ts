import { describe, expect, it } from 'vitest';

import { attached, mentionAt } from './mentions';

describe('an @ being typed', () => {
  it('is found at the start or after a space, with the word typed so far', () => {
    expect(mentionAt('@chec', 5)).toEqual({ start: 0, query: 'chec' });
    expect(mentionAt('why is @checkout slow', 16)).toEqual({ start: 7, query: 'checkout' });
    expect(mentionAt('why is @', 8)).toEqual({ start: 7, query: '' });
  });

  it('is not an e-mail address nor a word the caret has left', () => {
    expect(mentionAt('ops@example.com', 15)).toBeNull();
    expect(mentionAt('@checkout is slow', 17)).toBeNull();
  });

  it('knows what is attached already', () => {
    const references = [{ kind: 'incident' as const, id: 'inc-1', label: 'API 502s' }];
    expect(attached(references, { kind: 'incident', id: 'inc-1', label: '' })).toBe(true);
    expect(attached(references, { kind: 'alert_rule', id: 'inc-1', label: '' })).toBe(false);
  });
});
