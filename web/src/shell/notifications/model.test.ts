import { describe, expect, it } from 'vitest';

import type { Incident } from '@/types/alerting';

import { byUrgency, countUnread, isWaitingOn, parseReadNotifications, settleRead, type AppNotification } from './model';

function incident(fields: Partial<Incident>): Incident {
  return { status: 'open', assignees: [], severity: 'error', createdAt: 1, ...fields } as Incident;
}

const note = (id: string): AppNotification => ({ id, category: 'alerts', title: id });

describe('isWaitingOn', () => {
  it('rings for what is unanswered and mine, or nobody’s', () => {
    expect(isWaitingOn(incident({}), 'me')).toBe(true);
    expect(isWaitingOn(incident({ assignees: ['me'] }), 'me')).toBe(true);
  });

  it('stays quiet about what someone else holds, what was answered and what is snoozed', () => {
    expect(isWaitingOn(incident({ assignees: ['other'] }), 'me')).toBe(false);
    expect(isWaitingOn(incident({ status: 'acknowledged' }), 'me')).toBe(false);
    expect(isWaitingOn(incident({ status: 'resolved' }), 'me')).toBe(false);
    expect(isWaitingOn(incident({ snoozedUntil: 5 }), 'me')).toBe(false);
  });
});

describe('byUrgency', () => {
  it('takes the worst first, then the one waiting longest', () => {
    const list = [
      incident({ id: 'warn', severity: 'warning', createdAt: 1 }),
      incident({ id: 'late', severity: 'critical', createdAt: 9 }),
      incident({ id: 'early', severity: 'critical', createdAt: 2 }),
    ];
    expect(list.sort(byUrgency).map((item) => item.id)).toEqual(['early', 'late', 'warn']);
  });
});

describe('what was read', () => {
  it('counts what is listed and not read', () => {
    expect(countUnread([note('a'), note('b')], new Set(['a', 'gone']))).toBe(1);
  });

  it('forgets what left the list, so it does not grow without end', () => {
    expect(settleRead([note('a'), note('b')], new Set(['a', 'gone']), ['b'])).toEqual(['a', 'b']);
  });

  it('reads back only what it wrote', () => {
    expect(parseReadNotifications({ ids: ['a'] })).toEqual({ ids: ['a'] });
    expect(parseReadNotifications({ ids: [1] })).toBeNull();
    expect(parseReadNotifications('nonsense')).toBeNull();
  });
});
