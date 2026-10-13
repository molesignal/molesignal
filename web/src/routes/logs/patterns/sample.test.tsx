import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import * as React from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  logs: vi.fn(),
}));

vi.mock('@/api/web', () => ({
  logs: mocks.logs,
}));

import { PATTERN_SAMPLE_STEP, useLogPatternSample, type PatternSampleQuery } from './sample';

const QUERY: PatternSampleQuery = {
  orgId: 'org-1',
  stream: 'app_logs',
  statement: "level = 'ERROR'",
  globalFilters: [],
  timeRange: { start: 100, end: 200 },
  ranAt: 1,
  step: PATTERN_SAMPLE_STEP,
};

/** An answer of the list endpoint: `size` logs, and the cursor of the next one if there is one. */
function answer(index: number, size: number, next: boolean) {
  return {
    items: Array.from({ length: size }, (_, item) => ({ _timestamp: 1_000_000 - index * 10_000 - item, message: 'ok' })),
    next_cursor: next ? `cursor-${index + 1}` : null,
    previous_cursor: null,
    has_more: next,
  };
}

/**
 * A stream with more logs than will ever be read: every answer names a next one. It gives
 * what it is asked for, up to `cap` logs in one answer.
 */
function endless(cap = Number.POSITIVE_INFINITY) {
  mocks.logs.mockImplementation((request: { cursor?: string; limit: number }) => {
    const index = request.cursor ? Number(request.cursor.replace('cursor-', '')) : 0;

    return Promise.resolve(answer(index, Math.min(request.limit, cap), true));
  });
}

function renderSample(initial: PatternSampleQuery | null = QUERY) {
  const client = new QueryClient();
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );

  return renderHook(({ query }) => useLogPatternSample(query), { wrapper, initialProps: { query: initial } });
}

const read = (result: { rows: unknown[] } | undefined) => result?.rows.length ?? 0;

describe('useLogPatternSample', () => {
  beforeEach(() => {
    mocks.logs.mockReset();
  });

  it('reads the latest few hundred logs of the list query in one answer', async () => {
    endless();
    const { result } = renderSample();

    expect(result.current.reading).toBe(true);
    await waitFor(() => expect(result.current.reading).toBe(false));

    expect(read(result.current.result)).toBe(PATTERN_SAMPLE_STEP);
    expect(result.current.more).toBe(true);
    expect(result.current.failed).toBe(false);
    // Asked as the list asks its first page, for a step of logs.
    expect(mocks.logs).toHaveBeenCalledTimes(1);
    expect(mocks.logs).toHaveBeenCalledWith({
      stream: 'app_logs',
      from: 100,
      to: 200,
      filters: [{ field: 'level', op: '=', value: 'ERROR', quoted: true }],
      free_text: [],
      limit: PATTERN_SAMPLE_STEP,
    });
  });

  it('asks again by the cursor where an answer holds fewer logs than a step', async () => {
    // A backend that caps an answer at a hundred, whatever is asked.
    endless(100);
    const { result } = renderSample();

    await waitFor(() => expect(result.current.reading).toBe(false));

    expect(read(result.current.result)).toBe(PATTERN_SAMPLE_STEP);
    expect(mocks.logs).toHaveBeenCalledTimes(5);
    expect(mocks.logs.mock.calls.slice(1).map(([request]) => request)).toEqual([
      { cursor: 'cursor-1', limit: PATTERN_SAMPLE_STEP },
      { cursor: 'cursor-2', limit: PATTERN_SAMPLE_STEP },
      { cursor: 'cursor-3', limit: PATTERN_SAMPLE_STEP },
      { cursor: 'cursor-4', limit: PATTERN_SAMPLE_STEP },
    ]);
  });

  it('reads by as much as a page of the list where the reader pages by more than a step', async () => {
    endless();
    const { result } = renderSample({ ...QUERY, step: 2_000 });

    await waitFor(() => expect(result.current.reading).toBe(false));

    expect(read(result.current.result)).toBe(2_000);
    expect(mocks.logs).toHaveBeenCalledTimes(1);
    expect(mocks.logs.mock.calls[0]?.[0]).toMatchObject({ limit: 2_000 });

    act(() => result.current.readMore());
    await waitFor(() => expect(read(result.current.result)).toBe(4_000));
  });

  it('reads further back only when asked', async () => {
    endless();
    const { result } = renderSample();
    await waitFor(() => expect(result.current.reading).toBe(false));

    act(() => result.current.readMore());
    await waitFor(() => expect(read(result.current.result)).toBe(2 * PATTERN_SAMPLE_STEP));
    await waitFor(() => expect(result.current.reading).toBe(false));

    expect(mocks.logs).toHaveBeenCalledTimes(2);
    expect(mocks.logs.mock.calls[1]?.[0]).toEqual({ cursor: 'cursor-1', limit: PATTERN_SAMPLE_STEP });
  });

  it('stops where the logs that match end', async () => {
    mocks.logs
      .mockResolvedValueOnce(answer(0, 100, true))
      .mockResolvedValueOnce(answer(1, 37, false));
    const { result } = renderSample();

    await waitFor(() => expect(result.current.reading).toBe(false));

    expect(read(result.current.result)).toBe(137);
    expect(result.current.more).toBe(false);
    expect(mocks.logs).toHaveBeenCalledTimes(2);
  });

  it('gives what it read as one result, a field only an older log carries among its columns', async () => {
    mocks.logs
      .mockResolvedValueOnce({ ...answer(0, 2, true), items: [{ _timestamp: 9, message: 'a' }, { _timestamp: 8, message: 'b' }] })
      .mockResolvedValueOnce({ ...answer(1, 1, false), items: [{ _timestamp: 7, message: 'c', trace_id: 't-1' }] });
    const { result } = renderSample();

    await waitFor(() => expect(result.current.reading).toBe(false));

    expect(result.current.result?.columns).toEqual(['_timestamp', 'message', 'trace_id']);
    expect(result.current.result?.rows).toEqual([
      [9, 'a', null],
      [8, 'b', null],
      [7, 'c', 't-1'],
    ]);
  });

  it('takes an answer with nothing in it for the end, whatever it says of a next one', async () => {
    mocks.logs
      .mockResolvedValueOnce(answer(0, 100, true))
      .mockResolvedValue(answer(1, 0, true));
    const { result } = renderSample();

    await waitFor(() => expect(result.current.reading).toBe(false));

    expect(result.current.more).toBe(false);
    expect(mocks.logs).toHaveBeenCalledTimes(2);
  });

  it('keeps what was read when an answer does not come, and asks for that answer again', async () => {
    mocks.logs
      .mockResolvedValueOnce(answer(0, 100, true))
      .mockRejectedValueOnce(new Error('gateway timeout'));
    const { result } = renderSample();

    await waitFor(() => expect(result.current.failed).toBe(true));
    expect(result.current.reading).toBe(false);
    expect(read(result.current.result)).toBe(100);

    mocks.logs.mockResolvedValue(answer(1, 20, false));
    act(() => result.current.retry());
    await waitFor(() => expect(result.current.failed).toBe(false));
    await waitFor(() => expect(result.current.reading).toBe(false));

    expect(read(result.current.result)).toBe(120);
    expect(mocks.logs.mock.calls.at(-1)?.[0]).toEqual({ cursor: 'cursor-1', limit: PATTERN_SAMPLE_STEP });
  });

  it('reads a query that is run again from its start', async () => {
    endless();
    const { result, rerender } = renderSample();
    await waitFor(() => expect(result.current.reading).toBe(false));
    act(() => result.current.readMore());
    await waitFor(() => expect(read(result.current.result)).toBe(2 * PATTERN_SAMPLE_STEP));

    rerender({ query: { ...QUERY, ranAt: 2 } });
    await waitFor(() => expect(result.current.reading).toBe(false));

    // How far the reader asked to read was of the query before.
    expect(read(result.current.result)).toBe(PATTERN_SAMPLE_STEP);
  });

  it('reads nothing without a query', () => {
    const { result } = renderSample(null);

    expect(result.current).toMatchObject({ result: undefined, reading: false, more: false, failed: false });
    expect(mocks.logs).not.toHaveBeenCalled();
  });
});
