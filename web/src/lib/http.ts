import axios, {
  AxiosError,
  type AxiosInstance,
  type AxiosRequestConfig,
  type InternalAxiosRequestConfig,
} from 'axios';

import { type AuthContextInput, sessionKey, useAuthStore } from '@/stores/auth';
import { useOrgStore } from '@/stores/useOrgStore';

/**
 * Normalized error shape every UI toast / store consumes. Avoids consumers
 * having to dig through `axios.response.data` shapes that differ per route.
 */
export interface ApiError {
  status: number;
  message: string;
  code?: string;
  /** Retry-After of a 429, in seconds. */
  retryAfter?: number;
  /** Backend X-Request-Id (echoed on every response); lets support correlate a
   *  user-reported failure with the exact server log line. */
  requestId?: string;
}

/** Read a header from an axios response (AxiosHeaders or plain). */
function readHeader(headers: unknown, name: string): string | undefined {
  if (!headers || typeof headers !== 'object') return undefined;
  const h = headers as { get?: (k: string) => unknown; [k: string]: unknown };
  const raw = typeof h.get === 'function' ? h.get(name) : h[name];
  return typeof raw === 'string' && raw ? raw : undefined;
}

function isApiError(err: unknown): err is ApiError {
  return (
    typeof err === 'object' &&
    err !== null &&
    typeof (err as ApiError).status === 'number' &&
    typeof (err as ApiError).message === 'string'
  );
}

export function toApiError(err: unknown): ApiError {
  // Already read once: a page that keeps the error it read passes it back here.
  if (!(err instanceof Error) && isApiError(err)) return err;
  if (err instanceof AxiosError) {
    const status = err.response?.status ?? 0;
    const data = err.response?.data as
      | { message?: string; error?: string; code?: string }
      | string
      | undefined;
    let message = err.message;
    let code: string | undefined;
    if (typeof data === 'string') {
      message = data;
    } else if (data && typeof data === 'object') {
      message = data.message ?? data.error ?? message;
      code = data.code;
    }
    const requestId = readHeader(err.response?.headers, 'x-request-id');
    const retryAfter = Number(readHeader(err.response?.headers, 'retry-after'));
    const result: ApiError = { status, message };
    if (code !== undefined) result.code = code;
    if (requestId) result.requestId = requestId;
    if (retryAfter > 0) result.retryAfter = retryAfter;
    return result;
  }
  return { status: 0, message: String((err as Error)?.message ?? err) };
}

export const http: AxiosInstance = axios.create({
  baseURL: '/api/v1',
  timeout: 30_000,
});

/**
 * The session this page shows, `<userId>:<orgId>`, sent with every request.
 * Every tab of the browser sends the same session cookie, so after another tab
 * switches organization or account the server refuses this page's requests
 * (409 `session_changed`) instead of running them in a session it does not show.
 */
export const SESSION_HEADER = 'X-MoleSignal-Session';

/** Paths that answer for the cookie's session whatever the page shows. */
const SESSION_PATH = '/auth/session';

/** Requests whose 401 is an answer about credentials, not an expired access token. */
const CREDENTIAL_PATHS = /^\/(auth\/(signin|signup|refresh|signout|forgot-password|reset-password|sso\/)|public\/)/;

function displayedSession(): string | null {
  const ctx = useAuthStore.getState().ctx;
  return ctx ? sessionKey(ctx) : null;
}

let refreshing: Promise<boolean> | null = null;

/**
 * Renews the access token with the refresh token in the browser's cookie.
 * Requests that fail at the same moment share one renewal.
 */
export function refreshSession(): Promise<boolean> {
  refreshing ??= axios
    .post('/api/v1/auth/refresh', null, { timeout: 30_000 })
    .then(
      () => true,
      () => false,
    )
    .finally(() => {
      refreshing = null;
    });
  return refreshing;
}

/** The session as `/auth/session` answers it; the store keeps the fields it knows. */
interface SessionView {
  userId: string;
  email?: string;
  displayName?: string;
  orgId: string;
  orgName?: string;
  displayRole: string;
  roles: AuthContextInput['roles'];
  scope?: unknown;
}

function sessionOf(view: SessionView): AuthContextInput {
  return {
    userId: view.userId,
    orgId: view.orgId,
    displayRole: view.displayRole,
    roles: view.roles,
    scope: view.scope,
    ...(view.email ? { email: view.email } : {}),
    ...(view.displayName ? { displayName: view.displayName } : {}),
    ...(view.orgName ? { orgName: view.orgName } : {}),
  };
}

/** The session the browser's cookie holds, or `null` when it holds none. */
export async function currentSession(): Promise<AuthContextInput | null> {
  const ask = async () => sessionOf((await axios.get<SessionView>(`/api/v1${SESSION_PATH}`, { timeout: 30_000 })).data);
  try {
    return await ask();
  } catch (error) {
    if ((error as AxiosError).response?.status !== 401 || !(await refreshSession())) return null;
    try {
      return await ask();
    } catch {
      return null;
    }
  }
}

let adopting: Promise<void> | null = null;

/**
 * Another tab changed the session: show the one the cookie holds now, on a
 * fresh page so nothing loaded for the old one stays on screen.
 */
export function adoptCurrentSession(): Promise<void> {
  adopting ??= currentSession().then((session) => {
    if (!session) {
      signOutHere();
      return;
    }
    useAuthStore.getState().setSession(session);
    useOrgStore.getState().reset();
    window.location.assign('/');
  }).finally(() => {
    adopting = null;
  });
  return adopting;
}

/** The session is gone: back to the sign-in page, then to where the user was. */
function signOutHere(): void {
  const next = encodeURIComponent(window.location.pathname + window.location.search);
  useAuthStore.getState().logout();
  useOrgStore.getState().reset();
  if (!window.location.pathname.startsWith('/signin')) {
    window.location.assign(`/signin?next=${next}`);
  }
}

function isSessionChanged(status: number | undefined, data: unknown): boolean {
  return status === 409 && (data as { error?: unknown } | null)?.error === 'session_changed';
}

http.interceptors.request.use((config) => {
  const displayed = displayedSession();
  if (displayed && config.url !== SESSION_PATH) config.headers.set(SESSION_HEADER, displayed);
  return config;
});

/**
 * A 401 means the access token expired: renew it once and send the request
 * again. When that fails too, the session is over (signed out elsewhere,
 * expired, account disabled) and the page goes to sign in. A 409
 * `session_changed` means another tab switched the session.
 */
http.interceptors.response.use(
  (resp) => resp,
  async (err: AxiosError) => {
    const config = err.config as (InternalAxiosRequestConfig & { sessionRenewed?: boolean }) | undefined;
    const status = err.response?.status;
    const url = String(config?.url ?? '');

    if (isSessionChanged(status, err.response?.data)) {
      await adoptCurrentSession();
      return Promise.reject(err);
    }

    if (status === 401 && config && !CREDENTIAL_PATHS.test(url)) {
      if (!config.sessionRenewed && (await refreshSession())) {
        config.sessionRenewed = true;
        return http.request(config);
      }
      signOutHere();
    }
    return Promise.reject(err);
  },
);

/**
 * `fetch` for answers read as a stream, which axios does not hand over while
 * they arrive. Sends the page's session like `http` and renews an expired
 * access token once.
 */
export async function authFetch(url: string, init: RequestInit = {}): Promise<Response> {
  const send = () => {
    const headers = new Headers(init.headers);
    const displayed = displayedSession();
    if (displayed) headers.set(SESSION_HEADER, displayed);
    return fetch(url, { ...init, headers });
  };

  let response = await send();
  if (response.status === 401 && (await refreshSession())) response = await send();

  if (response.status === 401) {
    signOutHere();
  } else if (response.status === 409) {
    const body: unknown = await response
      .clone()
      .json()
      .catch(() => null);
    if (isSessionChanged(409, body)) await adoptCurrentSession();
  }
  return response;
}

/** What a read says in its address: absent values are left out. */
export type AddressParams = Record<string, string | number | boolean | null | undefined>;

/**
 * The longest address a read is asked at. A proxy in front of the server may
 * refuse a request line of more than 8 KB (nginx does by default), so a longer
 * read goes as a POST body instead.
 */
export const MAX_READ_ADDRESS = 6_000;

/**
 * A server older than its pages has no GET form of the read (405); a proxy may
 * still find the address too long (414). Either way the POST answers it.
 */
const GET_REFUSED = new Set([405, 414]);

/** The query string of a read's address. */
export function addressSearch(params: AddressParams): string {
  const search = new URLSearchParams();

  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null) search.set(key, String(value));
  }

  return search.toString();
}

/**
 * A read asked with GET, what it asks for in its address, so the request can
 * be copied and opened again as the page's own address can. A read too long for
 * an address, or one the server will not take with GET, is sent as a POST with
 * `body`, which says the same thing in the shape the POST takes.
 */
export async function getOrPost<T>(
  url: string,
  read: { params: AddressParams; body: unknown },
  config: AxiosRequestConfig = {},
): Promise<T> {
  const search = addressSearch(read.params);
  const address = search ? `${url}?${search}` : url;

  if ((http.defaults.baseURL ?? '').length + address.length <= MAX_READ_ADDRESS) {
    try {
      const { data } = await http.get<T>(address, config);
      return data;
    } catch (error) {
      if (!(error instanceof AxiosError) || !GET_REFUSED.has(error.response?.status ?? 0)) throw error;
    }
  }

  const { data } = await http.post<T>(url, read.body, config);
  return data;
}
