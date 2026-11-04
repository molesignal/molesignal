import { describe, expect, it } from 'vitest';

import type { ErrorSummary } from '@/api/apm';
import { patternMatches } from '@/api/apmErrorRules';

import { emptyRule, hasPattern, ruleFromGroup, ruleMatchesGroup } from './rules';

function group(errorType: string, message?: string): ErrorSummary {
  return {
    error: { fingerprint: 'fp', errorType: errorType, applicationFrame: 'app.Client.call', transactionName: 'GET /orders', overflow: false },
    service: { namespace: 'shop', name: 'checkout', environment: 'prod' },
    firstSeenAt: 1,
    lastSeenAt: 2,
    occurrenceCount: 3,
    ...(message ? { representativeMessage: message } : {}),
    red: {} as ErrorSummary['red'],
    traces: {} as ErrorSummary['traces'],
  };
}

describe('error rules', () => {
  it('matches the whole text, ignoring case, with stars for any text', () => {
    expect(patternMatches('*timeout*', 'ReadTimeoutError')).toBe(true);
    expect(patternMatches('a*b*c', 'aXXbYYc')).toBe(true);
    expect(patternMatches('abc', 'abcd')).toBe(false);
    expect(patternMatches('*', '')).toBe(true);
    expect(patternMatches('', 'a')).toBe(false);
  });

  it('takes the groups its service and patterns match', () => {
    const rule = { ...emptyRule(), errorType: '*Timeout*' };

    expect(ruleMatchesGroup(rule, group('SocketTimeoutException'))).toBe(true);
    expect(ruleMatchesGroup(rule, group('KeyError'))).toBe(false);
    expect(ruleMatchesGroup({ ...rule, serviceName: 'cart' }, group('SocketTimeoutException'))).toBe(false);
    expect(ruleMatchesGroup({ ...rule, message: '*upstream*' }, group('SocketTimeoutException'))).toBe(false);
    expect(ruleMatchesGroup({ ...rule, message: '*upstream*' }, group('SocketTimeoutException', 'Upstream slow'))).toBe(true);
    expect(ruleMatchesGroup(emptyRule(), group('KeyError'))).toBe(false);
  });

  it('starts a rule from a group with its type and service', () => {
    const rule = ruleFromGroup(group('KeyError'));

    expect(rule).toMatchObject({ name: 'KeyError', errorType: 'KeyError', serviceName: 'checkout', serviceNamespace: 'shop', action: 'merge' });
    expect(hasPattern(rule)).toBe(true);
    expect(hasPattern(emptyRule())).toBe(false);
  });
});
