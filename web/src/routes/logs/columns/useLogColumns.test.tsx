import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import * as React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useAuthStore } from '@/stores/auth';

const api = vi.hoisted(() => ({
  getUiPreference: vi.fn(),
  putUiPreference: vi.fn(),
  deleteUiPreference: vi.fn(),
}));
vi.mock('@/api/me', async (original) => ({ ...(await original<Record<string, unknown>>()), ...api }));

import {
  type LogColumnField,
  type LogColumnsState,
  NO_LOG_COLUMNS,
  parseKeptLogColumns,
  settleLogColumns,
  useLogColumns,
} from './useLogColumns';

const field = (name: string, count = 0, type = 'string'): LogColumnField => ({ name, type, count: String(count) });
const STREAM_FIELDS = [
  field('_timestamp', 0, 'timestamp'),
  field('level'),
  field('service'),
  field('message'),
  field('host'),
  field('env'),
  field('error', 0, 'boolean'),
];

describe('parseKeptLogColumns', () => {
  it('reads field names in their order, each once', () => {
    expect(parseKeptLogColumns({ fields: ['level', 'env', 'level'] })).toEqual({ fields: ['level', 'env'] });
    expect(parseKeptLogColumns({ fields: [] })).toEqual({ fields: [] });
  });

  it('takes anything else for nothing kept', () => {
    for (const raw of [null, undefined, 'level', ['level'], {}, { fields: 'level' }, { fields: ['level', 3] }]) {
      expect(parseKeptLogColumns(raw)).toBeNull();
    }
  });
});

describe('settleLogColumns', () => {
  const opened = () => settleLogColumns(NO_LOG_COLUMNS, 'app_logs', STREAM_FIELDS, null);

  it('opens a stream with its default when nothing is kept for it', () => {
    expect(opened()).toMatchObject({
      stream: 'app_logs',
      origin: 'default',
      visibleFields: ['level', 'service', 'message', 'host'],
    });
  });

  it('shows what is kept for the stream, as far as the stream still has it', () => {
    const kept = { fields: ['message', 'gone', 'env'] };

    expect(settleLogColumns(NO_LOG_COLUMNS, 'app_logs', STREAM_FIELDS, kept)).toMatchObject({
      origin: 'kept',
      visibleFields: ['message', 'env'],
    });
    // Kept columns arrive after the default was shown: they take its place.
    expect(settleLogColumns(opened(), 'app_logs', STREAM_FIELDS, kept).visibleFields).toEqual(['message', 'env']);
  });

  it('falls back to the default when none of the kept fields is left, but not when none was chosen', () => {
    expect(settleLogColumns(NO_LOG_COLUMNS, 'app_logs', STREAM_FIELDS, { fields: ['gone', 'also_gone'] })).toMatchObject({
      origin: 'default',
      visibleFields: ['level', 'service', 'message', 'host'],
    });
    expect(settleLogColumns(NO_LOG_COLUMNS, 'app_logs', STREAM_FIELDS, { fields: [] })).toMatchObject({
      origin: 'kept',
      visibleFields: [],
    });
  });

  it('lets what the reader arranged on this page stand, whatever is kept', () => {
    const arranged: LogColumnsState = { ...opened(), origin: 'reader', visibleFields: ['env', 'level'] };

    expect(settleLogColumns(arranged, 'app_logs', STREAM_FIELDS, { fields: ['message'] })).toBe(arranged);
    // A field the stream loses leaves the arrangement.
    const without = STREAM_FIELDS.filter((candidate) => candidate.name !== 'env');
    expect(settleLogColumns(arranged, 'app_logs', without, null).visibleFields).toEqual(['level']);
  });

  it('chooses the default once, and not again as more logs load', () => {
    const first = opened();
    const loaded = STREAM_FIELDS.map((candidate) => (candidate.name === 'env' ? field('env', 12) : candidate));

    expect(settleLogColumns(first, 'app_logs', loaded, null)).toBe(first);
  });

  it('opens on the fields the loaded logs have a value for, where any are loaded', () => {
    const loaded = [field('_timestamp', 5, 'timestamp'), field('level', 5), field('service'), field('message', 5), field('env', 5)];

    expect(settleLogColumns(NO_LOG_COLUMNS, 'app_logs', loaded, null).visibleFields).toEqual(['level', 'message', 'env']);
  });

  it('starts over for another stream', () => {
    const arranged: LogColumnsState = { ...opened(), origin: 'reader', visibleFields: ['env'] };
    const other = [field('_timestamp', 0, 'timestamp'), field('status'), field('request')];

    expect(settleLogColumns(arranged, 'nginx', other, null)).toMatchObject({
      stream: 'nginx',
      origin: 'default',
      visibleFields: ['status', 'request'],
    });
  });

  it('has no columns while nothing is known of the stream', () => {
    expect(settleLogColumns(opened(), 'app_logs', [], { fields: ['level'] })).toMatchObject({
      stream: 'app_logs',
      visibleFields: [],
    });
  });
});

describe('useLogColumns', () => {
  const wrapper = ({ children }: { children: React.ReactNode }) => {
    const [client] = React.useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }));

    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };

  beforeEach(() => {
    localStorage.clear();
    api.getUiPreference.mockReset().mockResolvedValue({ value: null, updated_at_micros: null });
    api.putUiPreference.mockReset().mockResolvedValue({ value: null, updated_at_micros: 1 });
    api.deleteUiPreference.mockReset().mockResolvedValue(undefined);
    useAuthStore.setState({ ctx: { user_id: 'user-1', org_id: 'org-1' } as never });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it('keeps an arrangement for the stream as soon as it is made, and forgets it on reset', async () => {
    const { result } = renderHook(() => useLogColumns('app_logs', STREAM_FIELDS), { wrapper });

    await waitFor(() => expect(result.current.visibleFields).toEqual(['level', 'service', 'message', 'host']));
    expect(result.current.arranged).toBe(false);
    expect(api.getUiPreference).toHaveBeenCalledWith('logs.columns', 'app_logs');

    act(() => result.current.toggle('host'));
    act(() => result.current.toggle('env'));
    expect(result.current.visibleFields).toEqual(['level', 'service', 'message', 'env']);
    expect(result.current.arranged).toBe(true);
    // Sent a moment later, the last arrangement only.
    await waitFor(() => expect(api.putUiPreference).toHaveBeenCalledTimes(1));
    expect(api.putUiPreference).toHaveBeenCalledWith('logs.columns', 'app_logs', {
      fields: ['level', 'service', 'message', 'env'],
    });

    act(() => result.current.reset());
    await waitFor(() => expect(result.current.visibleFields).toEqual(['level', 'service', 'message', 'host']));
    expect(result.current.arranged).toBe(false);
    expect(api.deleteUiPreference).toHaveBeenCalledWith('logs.columns', 'app_logs');
  });

  it('opens on what the server keeps for the stream', async () => {
    api.getUiPreference.mockResolvedValue({ value: { fields: ['message', 'env'] }, updated_at_micros: 9 });

    const { result } = renderHook(() => useLogColumns('app_logs', STREAM_FIELDS), { wrapper });

    await waitFor(() => expect(result.current.visibleFields).toEqual(['message', 'env']));
    expect(result.current.arranged).toBe(true);
    expect(api.putUiPreference).not.toHaveBeenCalled();
  });

  it('opens at once on what this browser remembers, before the server has answered', () => {
    api.getUiPreference.mockReturnValue(new Promise(() => undefined));
    const { result: first, unmount } = renderHook(() => useLogColumns('app_logs', STREAM_FIELDS), { wrapper });
    act(() => first.current.reorder('message', 'level', 'before'));
    unmount();

    const { result } = renderHook(() => useLogColumns('app_logs', STREAM_FIELDS), { wrapper });

    expect(result.current.visibleFields).toEqual(['message', 'level', 'service', 'host']);
  });
});
