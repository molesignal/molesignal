import { afterEach, describe, expect, it, vi } from 'vitest';

import { http } from '@/lib/http';

import { list } from './streams';

const stream = {
  id: 'stream-1',
  name: 'app_logs',
  streamType: 'logs',
  schema: { fields: [] },
  retention: { days: 7 },
  effectiveRetention: { days: 7 },
  settings: {},
  createdAt: 1,
  updatedAt: 2,
} as const;

describe('streams API list response', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('accepts a bare array response', async () => {
    vi.spyOn(http, 'get').mockResolvedValue({ data: [stream] } as never);

    await expect(list()).resolves.toMatchObject([{ id: 'stream-1', name: 'app_logs' }]);
  });

  it('accepts an items envelope response', async () => {
    vi.spyOn(http, 'get').mockResolvedValue({ data: { items: [stream] } } as never);

    await expect(list()).resolves.toMatchObject([{ id: 'stream-1', name: 'app_logs' }]);
  });

  it('returns an empty list for a malformed response instead of throwing', async () => {
    vi.spyOn(http, 'get').mockResolvedValue({ data: {} } as never);

    await expect(list()).resolves.toEqual([]);
  });
});
