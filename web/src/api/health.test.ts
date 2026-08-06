import { afterEach, describe, expect, it, vi } from 'vitest';

import { get } from '@/api/health';
import { http } from '@/lib/http';

describe('health API', () => {
  afterEach(() => vi.restoreAllMocks());

  it('returns a successful health response', async () => {
    vi.spyOn(http, 'get').mockResolvedValue({
      data: { status: 'ok' },
      status: 200,
    } as never);

    await expect(get()).resolves.toEqual({ status: 'ok' });
  });

  it('preserves degraded details while marking a 503 as failed', async () => {
    vi.spyOn(http, 'get').mockResolvedValue({
      data: { status: 'degraded', reason: 'storage unavailable' },
      status: 503,
    } as never);

    await expect(get()).rejects.toMatchObject({
      name: 'DegradedSystemHealthError',
      health: {
        status: 'degraded',
        reason: 'storage unavailable',
      },
    });
  });
});
