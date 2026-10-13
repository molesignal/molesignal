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
  next_cursor: null,
  previous_cursor: null,
  has_more: false,
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
      globalFilters: [],
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
      free_text: [],
      limit: 20,
    });
  });

  it('asks the list for a page of exactly what the histogram is asked to count', async () => {
    const globalFilters = [{ key: 'env', operator: '=' as const, value: 'prod' }];
    const timeRange = { start: 100, end: 200 };

    await runLogCursorQuery({
      stream: 'app_logs',
      statement: "service = 'api' AND timeout",
      globalFilters,
      timeRange,
      pageSize: 50,
    });

    const matched = logMatchRequest('app_logs', "service = 'api' AND timeout", globalFilters, timeRange);
    expect(matched).toEqual({
      stream: 'app_logs',
      from: 100,
      to: 200,
      filters: [
        { field: 'service', op: '=', value: 'api', quoted: true },
        { field: 'env', op: '=', value: 'prod', quoted: true },
      ],
      free_text: ['timeout'],
    });
    expect(mocks.logs).toHaveBeenCalledWith({ ...matched, limit: 50 });
  });

  it('does not execute a malformed function as an unfiltered query', async () => {
    await expect(runLogCursorQuery({
      stream: 'app_logs',
      statement: 'MATCH(level, INFO)',
      globalFilters: [],
      timeRange: { start: 100, end: 200 },
      pageSize: 20,
    })).rejects.toThrow('Invalid Fields query');

    expect(mocks.logs).not.toHaveBeenCalled();
  });
});
