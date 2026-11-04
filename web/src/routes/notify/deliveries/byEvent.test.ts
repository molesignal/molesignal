import { describe, expect, it } from 'vitest';

import type { NotifyDelivery } from '@/api/notify';

import { groupByEvent } from './byEvent';

function delivery(fields: Partial<NotifyDelivery>): NotifyDelivery {
  return { id: 'd', eventId: 'e1', attempt: 1, status: 'success', createdAt: 100, connectorId: 'pager', ...fields } as NotifyDelivery;
}

describe('groupByEvent', () => {
  it('gathers the tries of one alert by channel, oldest first', () => {
    const [event] = groupByEvent([
      delivery({ id: 'b', attempt: 2, createdAt: 300, connectorId: 'email' }),
      delivery({ id: 'a', attempt: 1, createdAt: 200, connectorId: 'email', status: 'failed' }),
      delivery({ id: 'c', createdAt: 100, connectorId: 'pager' }),
    ]);

    expect(event?.channels.map((channel) => channel.connectorId)).toEqual(['pager', 'email']);
    expect(event?.channels[1]?.attempts.map((attempt) => attempt.id)).toEqual(['a', 'b']);
    expect(event?.failed).toBe(1);
  });

  it('measures from the first notification to the first that arrived', () => {
    const [event] = groupByEvent([
      delivery({ id: 'a', createdAt: 100, status: 'failed' }),
      delivery({ id: 'b', createdAt: 150, sentAt: 170 }),
      delivery({ id: 'c', createdAt: 160, sentAt: 500, connectorId: 'email' }),
    ]);

    expect(event?.firstDeliveredAfter).toBe(70);
  });

  it('says nothing arrived when nothing did', () => {
    const [event] = groupByEvent([delivery({ status: 'failed' }), delivery({ status: 'pending', id: 'b' })]);
    expect(event?.firstDeliveredAfter).toBeNull();
  });

  it('lists the newest alert first', () => {
    const events = groupByEvent([
      delivery({ eventId: 'old', createdAt: 1 }),
      delivery({ eventId: 'new', createdAt: 9 }),
    ]);
    expect(events.map((event) => event.eventId)).toEqual(['new', 'old']);
  });
});
