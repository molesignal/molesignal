import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  post: vi.fn(),
}));

vi.mock('@/lib/http', () => ({
  http: { post: mocks.post },
  toApiError: (error: unknown) => ({ message: String(error) }),
}));

import { getSession } from './rum';

describe('RUM API presentation', () => {
  beforeEach(() => {
    mocks.post.mockReset();
  });

  it('normalizes common session IP aliases for the detail view', async () => {
    mocks.post
      .mockResolvedValueOnce({
        data: {
          columns: ['session_id', 'client_ip'],
          rows: [['session-1', '203.0.113.42']],
          scanned_rows: 1,
          took_ms: 1,
        },
      })
      .mockResolvedValueOnce({
        data: {
          columns: [],
          rows: [],
          scanned_rows: 0,
          took_ms: 1,
        },
      });

    const result = await getSession({
      org_id: 'org-1',
      session_id: 'session-1',
      from_micros: 1,
      to_micros: 2,
    });

    expect(result.session?.ip_address).toBe('203.0.113.42');
  });
});
