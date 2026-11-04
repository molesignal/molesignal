import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  logs: vi.fn(),
}));

vi.mock('@/api/web', () => ({
  logs: mocks.logs,
}));

import { logMatchRequest, runLogCursorQuery } from './cursorQuery';

const EMPTY_PAGE = {
  items: [],
  nextCursor: null,
  previousCursor: null,
  hasMore: false,
};

describe('log cursor query', () => {
  beforeEach(() => {
    mocks.logs.mockReset();
    mocks.logs.mockResolvedValue(EMPTY_PAGE);
  });

  it('sends MATCH as a structured cursor filter', async () => {
    await runLogCursorQuery({
      stream: 'app_logs',
      statement: "MATCH(level, 'INFO')",
      pinned: [],
      timeRange: { start: 100, end: 200 },
      pageSize: 20,
    });

    expect(mocks.logs).toHaveBeenCalledWith({
      stream: 'app_logs',
      from: 100,
      to: 200,
      filters: [{
        field: 'level',
        op: 'match',
        value: 'INFO',
        quoted: true,
      }],
      freeText: [],
      limit: 20,
    });
  });

  it('asks the list for a page of exactly what the histogram is asked to count', async () => {
    const pinned = [{ field: 'env', operator: '=' as const, value: 'prod' }];
    const timeRange = { start: 100, end: 200 };

    await runLogCursorQuery({
      stream: 'app_logs',
      statement: "service = 'api' AND timeout",
      pinned,
      timeRange,
      pageSize: 50,
    });

    const matched = logMatchRequest('app_logs', "service = 'api' AND timeout", pinned, timeRange);
    expect(matched).toEqual({
      stream: 'app_logs',
      from: 100,
      to: 200,
      filters: [
        { field: 'service', op: '=', value: 'api', quoted: true },
        { field: 'env', op: '=', value: 'prod', quoted: true },
      ],
      freeText: ['timeout'],
    });
    expect(mocks.logs).toHaveBeenCalledWith({ ...matched, limit: 50 });
  });

  it('does not execute a malformed function as an unfiltered query', async () => {
    await expect(runLogCursorQuery({
      stream: 'app_logs',
      statement: 'MATCH(level, INFO)',
      pinned: [],
      timeRange: { start: 100, end: 200 },
      pageSize: 20,
    })).rejects.toThrow('Invalid Fields query');

    expect(mocks.logs).not.toHaveBeenCalled();
  });

  it('sends alternatives as a condition tree beside the lists', () => {
    const pinned = [
      { field: 'service', operator: '=' as const, value: 'pinned' },
      { field: 'env', operator: '=' as const, value: 'prod' },
    ];

    expect(
      logMatchRequest(
        'app_logs',
        "level = 'error' AND (service = 'checkout' OR \"connection reset\") AND timeout",
        pinned,
        { start: 100, end: 200 },
      ),
    ).toEqual({
      stream: 'app_logs',
      from: 100,
      to: 200,
      // `service` is the reader's own: the pinned one gives way to the alternatives that speak of it.
      filters: [
        { field: 'level', op: '=', value: 'error', quoted: true },
        { field: 'env', op: '=', value: 'prod', quoted: true },
      ],
      freeText: ['timeout'],
      where: {
        or: [{ field: 'service', op: '=', value: 'checkout', quoted: true }, { text: 'connection reset' }],
      },
    });
  });

  it('joins several alternatives with AND, AND before OR inside each', () => {
    expect(
      logMatchRequest('app_logs', "(a = 1 OR b = 2) AND (c = 3 OR d = 4 AND x)", [], { start: 1, end: 2 }).where,
    ).toEqual({
      and: [
        { or: [{ field: 'a', op: '=', value: '1', quoted: false }, { field: 'b', op: '=', value: '2', quoted: false }] },
        {
          or: [
            { field: 'c', op: '=', value: '3', quoted: false },
            { and: [{ field: 'd', op: '=', value: '4', quoted: false }, { text: 'x' }] },
          ],
        },
      ],
    });
  });

  it('runs no query with a part of an alternative it cannot read', () => {
    expect(() => logMatchRequest('app_logs', "level = 'error' OR MATCH(level, INFO)", [], { start: 1, end: 2 })).toThrow(
      'Invalid Fields query: MATCH(level, INFO)',
    );
  });
});
