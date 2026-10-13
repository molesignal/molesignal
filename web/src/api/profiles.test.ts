import { AxiosError, type AxiosResponse } from 'axios';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ get: vi.fn() }));

vi.mock('@/lib/http', async (importOriginal) => ({
  ...((await importOriginal()) as Record<string, unknown>),
  http: { get: mocks.get },
}));

import { flamegraph, list } from './profiles';

function failure(status: number, message: string): AxiosError {
  return new AxiosError(
    `Request failed with status code ${status}`,
    AxiosError.ERR_BAD_REQUEST,
    undefined,
    undefined,
    { status, data: { message } } as AxiosResponse,
  );
}

// The two ways the backend says the profiles stream has not been written to yet.
const NOT_WRITTEN_YET: Array<[string, AxiosError]> = [
  ['the query planner', failure(403, 'forbidden: stream not found: profiles')],
  [
    'the stream catalog',
    failure(404, 'not found: stream `profiles` of type `builtin.logs` does not exist'),
  ],
];

describe('profile reads before the first profile', () => {
  beforeEach(() => mocks.get.mockReset());

  it.each(NOT_WRITTEN_YET)('lists nothing when %s turns the read away', async (_source, error) => {
    mocks.get.mockRejectedValueOnce(error);

    await expect(list()).resolves.toEqual([]);
  });

  it.each(NOT_WRITTEN_YET)(
    'draws an empty flame graph when %s turns the read away',
    async (_source, error) => {
      mocks.get.mockRejectedValueOnce(error);

      await expect(flamegraph()).resolves.toMatchObject({
        profile_count: 0,
        truncated: false,
        flamebearer: { numTicks: 0, levels: [] },
      });
    },
  );

  it('still reports any other failure', async () => {
    const error = failure(500, 'internal error');

    mocks.get.mockRejectedValueOnce(error);
    await expect(list()).rejects.toBe(error);

    mocks.get.mockRejectedValueOnce(error);
    await expect(flamegraph()).rejects.toBe(error);
  });
});
