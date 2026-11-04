import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ getOrPost: vi.fn() }));

vi.mock('@/lib/http', async (importOriginal) => ({
  ...((await importOriginal()) as Record<string, unknown>),
  getOrPost: mocks.getOrPost,
}));

import { type LogListFilter, logs, logsFieldStats } from './web';

describe('log reads in an address', () => {
  beforeEach(() => mocks.getOrPost.mockReset());

  it('say their filters and the words to find as JSON lists, and keep the body for a POST', async () => {
    mocks.getOrPost.mockResolvedValue({ items: [] });
    const filters: LogListFilter[] = [{ field: 'level', op: '=', value: 'error' }];
    const read = { stream: 'app', from: 1, to: 2, filters, freeText: ['time out'], limit: 100 };

    await logs(read);

    expect(mocks.getOrPost).toHaveBeenCalledWith('/web/logs', {
      params: {
        stream: 'app',
        from: 1,
        to: 2,
        limit: 100,
        filters: JSON.stringify(filters),
        freeText: '["time out"]',
      },
      body: read,
    });
  });

  it('leave out lists that hold nothing', async () => {
    mocks.getOrPost.mockResolvedValue({ top: [] });

    await logsFieldStats({ stream: 'app', from: 1, to: 2, filters: [], field: 'level' });

    const [, { params }] = mocks.getOrPost.mock.calls[0]! as [string, { params: Record<string, unknown> }];
    expect(params).toEqual({ stream: 'app', from: 1, to: 2, field: 'level' });
    expect(Object.keys(params).filter((key) => params[key] !== undefined)).toEqual(['stream', 'from', 'to', 'field']);
  });
});
