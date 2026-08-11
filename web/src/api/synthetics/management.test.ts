import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
}));

vi.mock('@/lib/http', () => ({
  http: { get: mocks.get },
}));

import { listResultPage } from './management';

describe('Synthetics management API', () => {
  beforeEach(() => {
    mocks.get.mockReset();
  });

  it('passes result filters and page controls to the organization-scoped endpoint', async () => {
    mocks.get.mockResolvedValue({
      data: { items: [], total: 0, page: 2, per_page: 50 },
    });

    await listResultPage({
      query: 'checkout',
      outcome: 'failing',
      location_id: 'sin',
      page: 2,
      per_page: 50,
    });

    expect(mocks.get).toHaveBeenCalledWith('/synthetics/results', {
      params: {
        query: 'checkout',
        outcome: 'failing',
        location_id: 'sin',
        page: 2,
        per_page: 50,
      },
    });
  });
});
