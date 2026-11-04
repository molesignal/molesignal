import type { Request } from '@playwright/test';

/**
 * The reads of the explorers that are sent with GET, what they ask for in the
 * address, while it fits one (`getOrPost` in src/lib/http.ts), and as a POST
 * body when it does not.
 */
export type ExplorerRead = 'query' | 'web/logs' | 'web/logs/histogram' | 'web/logs/field-stats';

/** The params of the log reads that are numbers, those that are JSON lists, and the condition tree. */
const NUMBER_PARAMS = new Set(['from', 'to', 'limit', 'bucketTime', 'originTime']);
const LIST_PARAMS = new Set(['filters', 'freeText']);
const JSON_PARAMS = new Set([...LIST_PARAMS, 'where']);

/** A route for one of those reads: its address, with or without a query string. */
export function explorerRead(path: ExplorerRead): RegExp {
  return new RegExp(`/api/v1/${path.replace(/[/-]/g, '\\$&')}(?:\\?.*)?$`);
}

/** Whether a request is that read, by its path. */
export function isExplorerRead(request: Request, path: ExplorerRead): boolean {
  return new URL(request.url()).pathname === `/api/v1/${path}`;
}

/**
 * What a read asked for, in the shape of its POST body whichever way it was
 * sent: a query gets back its `timeRange` and its stream hint, a log read its
 * filters and the words to find.
 */
export function explorerReadBody<T = Record<string, unknown>>(request: Request): T {
  if (request.method() === 'POST') return request.postDataJSON() as T;

  const url = new URL(request.url());
  const params = url.searchParams;

  if (url.pathname.endsWith('/query')) {
    const stream = params.get('stream');
    const limit = params.get('limit');
    const step = params.get('step');

    return {
      language: params.get('language'),
      statement: params.get('statement'),
      timeRange: { start: Number(params.get('start')), end: Number(params.get('end')) },
      ...(stream ? { stream: { name: stream, streamType: params.get('streamType') } } : {}),
      ...(limit ? { limit: Number(limit) } : {}),
      ...(step ? { step: Number(step) } : {}),
    } as T;
  }

  const body: Record<string, unknown> = {};
  for (const [key, value] of params) {
    body[key] = JSON_PARAMS.has(key) ? JSON.parse(value) : NUMBER_PARAMS.has(key) ? Number(value) : value;
  }

  // An address leaves a list out when it holds nothing; a read of a stream sends it empty in a body.
  if ('stream' in body) {
    for (const key of LIST_PARAMS) body[key] ??= [];
  }

  return body as T;
}
