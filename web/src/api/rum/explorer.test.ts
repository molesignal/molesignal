import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ getOrPost: vi.fn() }));

vi.mock('@/lib/http', async (importOriginal) => ({
  ...((await importOriginal()) as Record<string, unknown>),
  getOrPost: mocks.getOrPost,
}));

import { facets, histogram, type RumFilter, search } from './explorer';

describe('the reads of the explorer', () => {
  beforeEach(() => mocks.getOrPost.mockReset());

  it('say the query in their address: the type, the range and the lists as JSON text, and keep the body for a POST', async () => {
    mocks.getOrPost.mockResolvedValue({ items: [] });
    const filters: RumFilter[] = [{ field: 'env', op: '=', value: 'prod', quoted: true }];
    const read = {
      type: 'views' as const,
      from: 10,
      to: 20,
      filters,
      freeText: ['timeout'],
      columns: ['view.url_path'],
      limit: 100,
    };

    await search(read);

    expect(mocks.getOrPost).toHaveBeenCalledWith('/rum/search', {
      params: {
        type: 'views',
        from: 10,
        to: 20,
        limit: 100,
        filters: JSON.stringify(filters),
        freeText: '["timeout"]',
        columns: '["view.url_path"]',
      },
      body: read,
    });
  });

  it('leave out lists that hold nothing', async () => {
    mocks.getOrPost.mockResolvedValue({ items: [] });

    await search({ type: 'errors', filters: [], freeText: [] });

    const [, { params }] = mocks.getOrPost.mock.calls[0]! as [string, { params: Record<string, unknown> }];

    expect(params).toEqual({ type: 'errors', filters: undefined, freeText: undefined, columns: undefined });
  });

  it('count values and bars with the same query', async () => {
    mocks.getOrPost.mockResolvedValue({});

    await facets({ type: 'resources', field: 'resource.type', from: 1, to: 2, limit: 10 });
    await histogram({ type: 'resources', from: 1, to: 2, bucketTime: 1_000, originTime: 0 });

    const [[facetsUrl, facetsRead], [histogramUrl, histogramRead]] = mocks.getOrPost.mock.calls as [
      [string, { params: Record<string, unknown> }],
      [string, { params: Record<string, unknown> }],
    ];

    expect(facetsUrl).toBe('/rum/facets');
    expect(facetsRead.params).toMatchObject({ type: 'resources', field: 'resource.type', limit: 10 });
    expect(histogramUrl).toBe('/rum/histogram');
    expect(histogramRead.params).toMatchObject({ bucketTime: 1_000, originTime: 0 });
  });
});
