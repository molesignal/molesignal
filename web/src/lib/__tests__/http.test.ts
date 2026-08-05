import { AxiosError, type AxiosAdapter, type AxiosRequestConfig, type AxiosResponse } from 'axios';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { http, toApiError } from '@/lib/http';
import { useAuthStore } from '@/stores/auth';
import { useOrgStore } from '@/stores/useOrgStore';

type Loc = { pathname: string; search: string };

/**
 * Replace axios's adapter with a function that returns whatever the test
 * stubs. We capture the merged request config so we can assert on the
 * Authorization header the request interceptor added.
 */
function stubAdapter(
  fn: (config: AxiosRequestConfig) => Promise<Partial<AxiosResponse>> | Partial<AxiosResponse>,
): { restore: () => void; lastConfig: () => AxiosRequestConfig | null } {
  const previous = http.defaults.adapter;
  let last: AxiosRequestConfig | null = null;
  const adapter: AxiosAdapter = async (config) => {
    last = config;
    const out = await fn(config);
    const status = out.status ?? 200;
    if (status >= 200 && status < 300) {
      return {
        data: out.data ?? null,
        status,
        statusText: out.statusText ?? 'OK',
        headers: out.headers ?? {},
        config,
      } as AxiosResponse;
    }
    throw new AxiosError(
      `Request failed with status ${status}`,
      String(status),
      config,
      undefined,
      {
        data: out.data,
        status,
        statusText: out.statusText ?? '',
        headers: out.headers ?? {},
        config,
      } as AxiosResponse,
    );
  };
  http.defaults.adapter = adapter;
  return {
    restore: () => {
      if (previous === undefined) {
        // axios accepts deleting the override to fall back to the default
        delete (http.defaults as Record<string, unknown>).adapter;
      } else {
        http.defaults.adapter = previous;
      }
    },
    lastConfig: () => last,
  };
}

function installLocation(loc: Loc): { assign: ReturnType<typeof vi.fn> } {
  const assign = vi.fn();
  Object.defineProperty(window, 'location', {
    configurable: true,
    value: { ...loc, assign },
  });
  return { assign };
}

describe('http interceptors', () => {
  let originalLocation: Location;

  beforeEach(() => {
    originalLocation = window.location;
    useAuthStore.setState({
      token: 'real-token',
      ctx: {
        user_id: 'u1',
        org_id: 'org-a',
        display_role: 'Owner',
        roles: [],
      },
    });
    useOrgStore.setState({
      orgs: [{ id: 'org-a', name: 'A', roles: [], disabled: false }],
      currentOrgId: 'org-a',
    });
  });

  afterEach(() => {
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: originalLocation,
    });
  });

  it('attaches bearer token from auth store', async () => {
    const a = stubAdapter(() => ({ status: 200, data: { ok: true } }));
    await http.get('/echo');
    expect(a.lastConfig()?.headers?.Authorization).toBe('Bearer real-token');
    a.restore();
  });

  it('on 401 clears auth/org and navigates to /signin with encoded next', async () => {
    const a = stubAdapter(() => ({ status: 401, data: { message: 'expired' } }));
    const { assign } = installLocation({
      pathname: '/investigate',
      search: '?time=-2h..now&q=foo bar',
    });

    await expect(http.get('/whatever')).rejects.toBeTruthy();

    expect(useAuthStore.getState().token).toBeNull();
    expect(useOrgStore.getState().orgs).toEqual([]);
    expect(assign).toHaveBeenCalledTimes(1);
    const url = assign.mock.calls[0]![0] as string;
    expect(url).toBe(
      `/signin?next=${encodeURIComponent('/investigate?time=-2h..now&q=foo bar')}`,
    );
    a.restore();
  });

});

describe('toApiError', () => {
  it('normalizes axios errors with object body', async () => {
    const a = stubAdapter(() => ({
      status: 422,
      data: { message: 'bad', code: 'invalid_x' },
    }));
    try {
      await http.get('/x');
      throw new Error('should have thrown');
    } catch (err) {
      const e = toApiError(err);
      expect(e.status).toBe(422);
      expect(e.message).toBe('bad');
      expect(e.code).toBe('invalid_x');
    }
    a.restore();
  });

  it('falls back to generic message for non-axios errors', () => {
    const e = toApiError(new Error('oh no'));
    expect(e.status).toBe(0);
    expect(e.message).toBe('oh no');
  });
});
