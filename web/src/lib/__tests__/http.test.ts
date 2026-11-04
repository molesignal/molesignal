import axios, {
  AxiosError,
  type AxiosAdapter,
  type AxiosHeaders,
  type AxiosRequestConfig,
  type AxiosResponse,
} from 'axios';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { getOrPost, http, MAX_READ_ADDRESS, SESSION_HEADER, toApiError } from '@/lib/http';
import { useAuthStore } from '@/stores/auth';
import { useOrgStore } from '@/stores/useOrgStore';

type Loc = { pathname: string; search: string };

/**
 * Replace axios's adapter with a function that returns whatever the test
 * stubs. We capture the merged request config so we can assert on the
 * headers the request interceptor added.
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
  let restoreAxios: () => void;

  /** Answers the requests of `http` and of the bare axios calls that renew the session. */
  function stubServer(answer: (url: string) => Partial<AxiosResponse>): { urls: string[] } {
    const urls: string[] = [];
    const stub = stubAdapter((config) => {
      const url = String(config.url);
      urls.push(url);
      return answer(url);
    });
    const previous = axios.defaults.adapter;
    axios.defaults.adapter = http.defaults.adapter!;
    restoreAxios = () => {
      if (previous === undefined) delete (axios.defaults as Record<string, unknown>).adapter;
      else axios.defaults.adapter = previous;
      stub.restore();
    };
    return { urls };
  }

  beforeEach(() => {
    originalLocation = window.location;
    restoreAxios = () => {};
    useAuthStore.setState({
      ctx: {
        userId: 'u1',
        orgId: 'org-a',
        displayRole: 'Owner',
        roles: [],
      },
    });
    useOrgStore.setState({
      orgs: [{ id: 'org-a', name: 'A', roles: [], disabled: false }],
      currentOrgId: 'org-a',
    });
  });

  afterEach(() => {
    restoreAxios();
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: originalLocation,
    });
  });

  it('says which session the page shows and sends no credential of its own', async () => {
    const a = stubAdapter(() => ({ status: 200, data: { ok: true } }));
    await http.get('/echo');
    const headers = a.lastConfig()?.headers as AxiosHeaders;
    expect(headers.get(SESSION_HEADER)).toBe('u1:org-a');
    expect(headers.get('Authorization')).toBeUndefined();
    a.restore();
  });

  it('renews an expired access token once and asks again', async () => {
    let expired = true;
    const server = stubServer((url) => {
      if (url === '/api/v1/auth/refresh') {
        expired = false;
        return { status: 204 };
      }
      return expired ? { status: 401 } : { status: 200, data: { ok: true } };
    });

    await expect(http.get('/whatever')).resolves.toMatchObject({ data: { ok: true } });
    expect(server.urls).toEqual(['/whatever', '/api/v1/auth/refresh', '/whatever']);
  });

  it('signs the page out when the session cannot be renewed', async () => {
    const server = stubServer(() => ({ status: 401, data: { message: 'expired' } }));
    const { assign } = installLocation({
      pathname: '/investigate',
      search: '?time=-2h..now&q=foo bar',
    });

    await expect(http.get('/whatever')).rejects.toBeTruthy();

    expect(server.urls).toEqual(['/whatever', '/api/v1/auth/refresh']);
    expect(useAuthStore.getState().ctx).toBeNull();
    expect(useOrgStore.getState().orgs).toEqual([]);
    expect(assign).toHaveBeenCalledTimes(1);
    const url = assign.mock.calls[0]![0] as string;
    expect(url).toBe(
      `/signin?next=${encodeURIComponent('/investigate?time=-2h..now&q=foo bar')}`,
    );
  });

  it('leaves a refused sign-in to the sign-in page', async () => {
    const server = stubServer(() => ({ status: 401, data: { message: 'invalid credentials' } }));
    const { assign } = installLocation({ pathname: '/signin', search: '' });

    await expect(http.post('/auth/signin', {})).rejects.toBeTruthy();

    expect(server.urls).toEqual(['/auth/signin']);
    expect(assign).not.toHaveBeenCalled();
  });

  it('opens the session another tab switched to on a fresh page', async () => {
    const server = stubServer((url) =>
      url === '/api/v1/auth/session'
        ? {
            status: 200,
            data: { userId: 'u1', orgId: 'org-b', displayRole: 'Viewer', roles: [] },
          }
        : { status: 409, data: { error: 'session_changed' } },
    );
    const { assign } = installLocation({ pathname: '/logs', search: '' });

    await expect(http.get('/whatever')).rejects.toBeTruthy();

    expect(server.urls).toEqual(['/whatever', '/api/v1/auth/session']);
    expect(useAuthStore.getState().ctx?.orgId).toBe('org-b');
    expect(assign).toHaveBeenCalledWith('/');
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

describe('getOrPost', () => {
  it('asks with GET, what it asks for in the address', async () => {
    const a = stubAdapter(() => ({ status: 200, data: { ok: true } }));

    await expect(
      getOrPost('/web/logs', {
        params: { stream: 'app', from: 1, cursor: undefined, filters: '[{"field":"level"}]' },
        body: { stream: 'app' },
      }),
    ).resolves.toEqual({ ok: true });

    expect(a.lastConfig()?.method).toBe('get');
    expect(a.lastConfig()?.url).toBe('/web/logs?stream=app&from=1&filters=%5B%7B%22field%22%3A%22level%22%7D%5D');
    a.restore();
  });

  it('sends a read too long for an address as a POST body', async () => {
    const a = stubAdapter(() => ({ status: 200, data: {} }));
    const statement = 'x'.repeat(MAX_READ_ADDRESS);

    await getOrPost('/query', { params: { statement }, body: { statement } });

    expect(a.lastConfig()?.method).toBe('post');
    expect(a.lastConfig()?.url).toBe('/query');
    expect(JSON.parse(String(a.lastConfig()?.data))).toEqual({ statement });
    a.restore();
  });

  it.each([405, 414])('asks again with POST when the server will not take the GET (%i)', async (status) => {
    const methods: string[] = [];
    const a = stubAdapter((config) => {
      methods.push(String(config.method));
      return config.method === 'get' ? { status } : { status: 200, data: { rows: [] } };
    });

    await expect(getOrPost('/query', { params: { statement: 'up' }, body: { statement: 'up' } })).resolves.toEqual({
      rows: [],
    });

    expect(methods).toEqual(['get', 'post']);
    a.restore();
  });

  it('does not ask again when the read itself is refused', async () => {
    const methods: string[] = [];
    const a = stubAdapter((config) => {
      methods.push(String(config.method));
      return { status: 400, data: { message: 'bad statement' } };
    });

    await expect(getOrPost('/query', { params: { statement: 'up(' }, body: {} })).rejects.toBeInstanceOf(AxiosError);

    expect(methods).toEqual(['get']);
    a.restore();
  });
});
