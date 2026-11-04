import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ getOrPost: vi.fn() }));

vi.mock('@/lib/http', async (importOriginal) => ({
  ...((await importOriginal()) as Record<string, unknown>),
  getOrPost: mocks.getOrPost,
}));

import { readQuery } from './query';

describe('readQuery', () => {
  beforeEach(() => mocks.getOrPost.mockReset());

  it('says the query flat in the address, as GET /query takes it, and keeps the body for a POST', async () => {
    mocks.getOrPost.mockResolvedValue({ columns: [], rows: [], scannedRows: 0, tookMs: 1 });
    const request = {
      orgId: 'org',
      language: 'sql' as const,
      statement: 'SELECT 1',
      timeRange: { start: 10, end: 20 },
      stream: { name: 'app', streamType: 'logs' as const },
      limit: 200,
    };

    await readQuery(request);

    expect(mocks.getOrPost).toHaveBeenCalledWith(
      '/query',
      {
        params: { language: 'sql', statement: 'SELECT 1', start: 10, end: 20, stream: 'app', streamType: 'logs', limit: 200 },
        body: request,
      },
      { headers: { Prefer: 'respond-sync' } },
    );
  });

  it('says the step of a PromQL range in the address: without it the answer is one instant', async () => {
    mocks.getOrPost.mockResolvedValue({ columns: [], rows: [], scannedRows: 0, tookMs: 1 });

    await readQuery({
      orgId: 'org',
      language: 'promql',
      statement: 'up',
      timeRange: { start: 0, end: 3_600_000_000 },
      limit: 50_000,
      step: 60_000_000,
    });

    expect(mocks.getOrPost.mock.calls[0]?.[1].params).toMatchObject({ limit: 50_000, step: 60_000_000 });
  });
});
