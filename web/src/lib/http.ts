import axios, { AxiosError, type AxiosInstance } from 'axios';

import { useAuthStore } from '@/stores/auth';
import { useOrgStore } from '@/stores/useOrgStore';

/**
 * Normalized error shape every UI toast / store consumes. Avoids consumers
 * having to dig through `axios.response.data` shapes that differ per route.
 */
export interface ApiError {
  status: number;
  message: string;
  code?: string;
  /** Backend X-Request-Id (echoed on every response); lets support correlate a
   *  user-reported failure with the exact server log line. */
  requestId?: string;
}

/** Read the X-Request-Id header from an axios response (AxiosHeaders or plain). */
function readRequestId(headers: unknown): string | undefined {
  if (!headers || typeof headers !== 'object') return undefined;
  const h = headers as { get?: (k: string) => unknown; [k: string]: unknown };
  const raw = typeof h.get === 'function' ? h.get('x-request-id') : h['x-request-id'];
  return typeof raw === 'string' && raw ? raw : undefined;
}

export function toApiError(err: unknown): ApiError {
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
    const requestId = readRequestId(err.response?.headers);
    const result: ApiError = { status, message };
    if (code !== undefined) result.code = code;
    if (requestId) result.requestId = requestId;
    return result;
  }
  return { status: 0, message: String((err as Error)?.message ?? err) };
}

export const http: AxiosInstance = axios.create({
  baseURL: '/api/v1',
  timeout: 30_000,
});

http.interceptors.request.use((config) => {
  const token = useAuthStore.getState().token;
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

/**
 * Single 401 handler — covers token-expiry on any axios call AND org-switch
 * failure (spec web-shell: "401 also fires on org-switch failure"). Clears
 * auth + org state, then redirects to `/signin?next=<pathname+search>`.
 */
http.interceptors.response.use(
  (resp) => resp,
  (err) => {
    if (err.response?.status === 401) {
      const next = encodeURIComponent(window.location.pathname + window.location.search);
      useAuthStore.getState().logout();
      useOrgStore.getState().reset();
      if (!window.location.pathname.startsWith('/signin')) {
        window.location.assign(`/signin?next=${next}`);
      }
    }
    return Promise.reject(err);
  },
);
