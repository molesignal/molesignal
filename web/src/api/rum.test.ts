import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
}));

vi.mock('@/lib/http', () => ({
  http: { get: mocks.get },
}));

import { getSession, listSessions } from './rum';

describe('RUM API presentation', () => {
  beforeEach(() => {
    mocks.get.mockReset();
  });

  it('loads an enriched session page with one dedicated request', async () => {
    mocks.get.mockResolvedValueOnce({
      data: {
        items: [
          {
            session_id: 'session-1',
            journey: ['/products', '/checkout'],
            rage_click_count: 1,
            experience: 'poor',
            replay_available: true,
          },
        ],
        next_cursor: null,
        previous_cursor: null,
        has_more: false,
      },
    });

    const result = await listSessions({
      org_id: 'org-1',
      from_micros: 1,
      to_micros: 2,
      limit: 20,
    });

    expect(result.items[0]).toMatchObject({
      session_id: 'session-1',
      journey: ['/products', '/checkout'],
      rage_click_count: 1,
      experience: 'poor',
      replay_available: true,
    });
    expect(mocks.get).toHaveBeenCalledTimes(1);
    expect(mocks.get).toHaveBeenCalledWith('/rum/sessions', {
      params: { from: 1, to: 2, limit: 20 },
    });
  });

  it('normalizes common session IP aliases for the detail view', async () => {
    mocks.get.mockResolvedValueOnce({
      data: {
        session: {
          session_id: 'session-1',
          client_ip: '203.0.113.42',
        },
        events: [],
      },
    });

    const result = await getSession({
      org_id: 'org-1',
      session_id: 'session-1',
      from_micros: 1,
      to_micros: 2,
    });

    expect(result.session?.ip_address).toBe('203.0.113.42');
    expect(mocks.get).toHaveBeenCalledWith('/rum/sessions/session-1', {
      params: { from: 1, to: 2 },
    });
  });
});
