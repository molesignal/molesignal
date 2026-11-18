import { describe, expect, it } from 'vitest';

import type { NotifyActionLink } from '@/api/notify/actions';

import { pageLanguage, refusalOf, settledRefusal, severityTone } from './model';

function link(patch: Partial<NotifyActionLink> = {}, status = 'open'): NotifyActionLink {
  return {
    action: 'acknowledge',
    alert: { id: 'a1', title: 'API unavailable', severity: 'critical', status, startedAt: 1 },
    expiresAt: 2,
    usedAt: null,
    language: 'zh-cn',
    ...patch,
  };
}

describe('a one-time action link page', () => {
  it('reads in the notification’s language, else the browser’s', () => {
    expect(pageLanguage('zh-cn', 'en-us')).toBe('zh-cn');
    expect(pageLanguage('ZH-CN', 'en-us')).toBe('zh-cn');
    expect(pageLanguage('fr-fr', 'zh-cn')).toBe('zh-cn');
    expect(pageLanguage(undefined, 'en-us')).toBe('en-us');
  });

  it('says why the server would not act, and lets other failures be tried again', () => {
    expect(refusalOf({ status: 409, message: 'x', code: 'used' })).toBe('used');
    expect(refusalOf({ status: 409, message: 'x', code: 'inactive' })).toBe('inactive');
    expect(refusalOf({ status: 403, message: 'x', code: 'no_access' })).toBe('no_access');
    expect(refusalOf({ status: 404, message: 'x', code: 'expired' })).toBe('expired');
    expect(refusalOf({ status: 404, message: 'not found' })).toBe('expired');
    expect(refusalOf({ status: 502, message: 'bad gateway' })).toBeNull();
    expect(refusalOf({ status: 0, message: 'Network Error' })).toBeNull();
  });

  it('offers no button for a link that was used or an alert that is over', () => {
    expect(settledRefusal(link())).toBeNull();
    expect(settledRefusal(link({}, 'acknowledged'))).toBeNull();
    expect(settledRefusal(link({ usedAt: 5 }))).toBe('used');
    expect(settledRefusal(link({}, 'resolved'))).toBe('inactive');
    expect(settledRefusal(link({}, 'closed'))).toBe('inactive');
  });

  it('colours the severity as the alert pages do', () => {
    expect(severityTone('critical')).toBe('red');
    expect(severityTone('warning')).toBe('yellow');
    expect(severityTone('unknown')).toBe('neutral');
  });
});
