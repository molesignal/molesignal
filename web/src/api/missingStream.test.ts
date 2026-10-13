import { AxiosError, type AxiosResponse } from 'axios';
import { describe, expect, it, vi } from 'vitest';

import { isMissingStreamError, whenStreamExists } from './missingStream';

function apiFailure(status: number, data: unknown): AxiosError {
  return new AxiosError(
    `Request failed with status code ${status}`,
    AxiosError.ERR_BAD_REQUEST,
    undefined,
    undefined,
    { status, data } as AxiosResponse,
  );
}

describe('isMissingStreamError', () => {
  it('recognises the stream catalog turning a read away', () => {
    expect(
      isMissingStreamError(
        apiFailure(404, {
          message:
            'not found: stream `rum_sessions` of type `builtin.logs` does not exist',
        }),
      ),
    ).toBe(true);
    expect(
      isMissingStreamError(apiFailure(404, 'stream `profiles` does not exist')),
    ).toBe(true);
  });

  it('recognises the query planner turning a read away', () => {
    expect(
      isMissingStreamError(
        apiFailure(403, { error: 'forbidden: stream not found: rum_errors' }),
      ),
    ).toBe(true);
    expect(isMissingStreamError(new Error('Unknown stream "default"'))).toBe(true);
  });

  it('leaves every other failure alone', () => {
    expect(
      isMissingStreamError(apiFailure(404, { message: 'session not found' })),
    ).toBe(false);
    expect(
      isMissingStreamError(apiFailure(404, { message: 'file does not exist' })),
    ).toBe(false);
    expect(
      isMissingStreamError(apiFailure(500, { message: 'internal error' })),
    ).toBe(false);
    expect(isMissingStreamError(new Error('Network Error'))).toBe(false);
    expect(isMissingStreamError(undefined)).toBe(false);
  });
});

describe('whenStreamExists', () => {
  it('passes the answer through', async () => {
    const empty = vi.fn(() => [] as number[]);

    await expect(whenStreamExists(async () => [1, 2], empty)).resolves.toEqual([
      1, 2,
    ]);
    expect(empty).not.toHaveBeenCalled();
  });

  it('answers a fresh empty value when the stream is not there yet', async () => {
    const read = () =>
      Promise.reject(
        apiFailure(404, {
          message:
            'not found: stream `rum_sessions` of type `builtin.logs` does not exist',
        }),
      );

    const first = await whenStreamExists<number[]>(read, () => []);
    const second = await whenStreamExists<number[]>(read, () => []);

    expect(first).toEqual([]);
    expect(second).not.toBe(first);
  });

  it('lets other failures through untouched', async () => {
    const failure = apiFailure(500, { message: 'internal error' });

    await expect(
      whenStreamExists(() => Promise.reject(failure), () => []),
    ).rejects.toBe(failure);
  });
});
