import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ getOrPost: vi.fn() }));

vi.mock('@/lib/http', async (importOriginal) => ({
  ...((await importOriginal()) as Record<string, unknown>),
  getOrPost: mocks.getOrPost,
}));

import { aggregate, type RumMeasure } from './aggregate';

describe('the aggregate read', () => {
  beforeEach(() => mocks.getOrPost.mockReset());

  it('says the selection and the measures in its address, and keeps the body for a POST', async () => {
    mocks.getOrPost.mockResolvedValue({ rows: [] });
    const measures: RumMeasure[] = [
      { fn: 'count', name: 'views' },
      { fn: 'p75', field: 'view.performance.lcp.timestamp', where: [{ field: 'env', op: '=', value: 'prod' }] },
    ];
    const read = {
      type: 'views' as const,
      from: 10,
      to: 20,
      measures,
      groupBy: ['view.url_path_group'],
      limit: 8,
      orderBy: 'views',
      order: 'desc' as const,
    };

    await aggregate(read);

    expect(mocks.getOrPost).toHaveBeenCalledWith(
      '/rum/aggregate',
      {
        params: {
          type: 'views',
          from: 10,
          to: 20,
          limit: 8,
          orderBy: 'views',
          order: 'desc',
          filters: undefined,
          freeText: undefined,
          measures: JSON.stringify(measures),
          groupBy: '["view.url_path_group"]',
        },
        body: read,
      },
      {},
    );
  });

  it('leaves out lists that hold nothing and passes the signal on', async () => {
    mocks.getOrPost.mockResolvedValue({ rows: [] });
    const signal = new AbortController().signal;

    await aggregate({ type: 'sessions', measures: [{ fn: 'count' }], groupBy: [], filters: [] }, signal);

    const [, { params }, config] = mocks.getOrPost.mock.calls[0]! as [
      string,
      { params: Record<string, unknown> },
      { signal: AbortSignal },
    ];

    expect(params).toMatchObject({ filters: undefined, groupBy: undefined, measures: '[{"fn":"count"}]' });
    expect(config.signal).toBe(signal);
  });
});
