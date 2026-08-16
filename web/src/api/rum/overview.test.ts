import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ get: vi.fn() }));

vi.mock('@/lib/http', () => ({ http: { get: mocks.get } }));

import { getOverview, getOverviewInsights } from './overview';

describe('RUM overview read model', () => {
  beforeEach(() => mocks.get.mockReset());

  it('uses one dedicated request and forwards the active scope', async () => {
    const data = {
      metrics: {
        users: 3,
        sessions: 4,
        errorFreeRate: 0.75,
        lcpP75: 2_500,
        inpP75: 180,
        clsP75: 0.05,
      },
      browserDevices: [],
      regions: [],
      facets: {
        applications: ['storefront'],
        environments: ['prod'],
        versions: ['2.0.0'],
        countries: ['CN'],
        devices: ['desktop'],
      },
    };
    mocks.get.mockResolvedValueOnce({ data });

    await expect(
      getOverview({
        org_id: 'org-1',
        from_micros: 10,
        to_micros: 20,
        application: 'storefront',
        environment: 'prod',
      }),
    ).resolves.toEqual(data);

    expect(mocks.get).toHaveBeenCalledWith('/rum/overview', {
      params: {
        from: 10,
        to: 20,
        application: 'storefront',
        environment: 'prod',
      },
    });
  });

  it('requests only metrics for the previous period', async () => {
    mocks.get.mockResolvedValueOnce({ data: {} });
    await getOverview({
      org_id: 'org-1',
      from_micros: 1,
      to_micros: 10,
      summary_only: true,
    });

    expect(mocks.get.mock.calls[0]?.[1]).toEqual({
      params: { from: 1, to: 10, summary_only: true },
    });
  });

  it('loads expensive insights from a separate endpoint', async () => {
    const data = {
      trend: [],
      satisfaction: { good: 3, needsImprovement: 0, poor: 1, total: 4 },
      slowPages: [],
      frequentErrors: [],
    };
    mocks.get.mockResolvedValueOnce({ data });

    await expect(
      getOverviewInsights({
        org_id: 'org-1',
        from_micros: 10,
        to_micros: 20,
        device: 'desktop',
      }),
    ).resolves.toEqual(data);

    expect(mocks.get).toHaveBeenCalledWith('/rum/overview/insights', {
      params: { from: 10, to: 20, device: 'desktop' },
    });
  });
});
