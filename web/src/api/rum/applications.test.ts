import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
}));

vi.mock('@/lib/http', () => ({
  http: { get: mocks.get },
}));

import { listApplicationSummaries } from './applications';

describe('RUM application summaries', () => {
  beforeEach(() => {
    mocks.get.mockReset();
  });

  it('uses the dedicated compact read-model endpoint', async () => {
    const items = [
      {
        application: 'storefront',
        environments: ['prod'],
        versions: ['2.0.0'],
        users: 8,
        sessions: 12,
        errorFreeRate: 0.75,
        lcpP75: 2_500,
      },
    ];
    mocks.get.mockResolvedValueOnce({ data: { items } });

    await expect(
      listApplicationSummaries({
        org_id: 'org-1',
        from_micros: 10,
        to_micros: 20,
      }),
    ).resolves.toEqual(items);

    expect(mocks.get).toHaveBeenCalledWith('/rum/applications/summary', {
      params: { from: 10, to: 20 },
    });
  });

  it('pushes environment and version filters into the endpoint request', async () => {
    mocks.get.mockResolvedValueOnce({ data: { items: [] } });

    await listApplicationSummaries({
      org_id: 'org-1',
      from_micros: 10,
      to_micros: 20,
      environment: "prod'west",
      version: '2.0.0',
    });

    expect(mocks.get).toHaveBeenCalledWith('/rum/applications/summary', {
      params: {
        from: 10,
        to: 20,
        environment: "prod'west",
        version: '2.0.0',
      },
    });
  });
});
