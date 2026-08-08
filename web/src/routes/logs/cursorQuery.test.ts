import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  logs: vi.fn(),
}));

vi.mock('@/api/web', () => ({
  logs: mocks.logs,
}));

import { runLogCursorQuery } from './cursorQuery';

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
