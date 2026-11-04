import { useQuery } from '@tanstack/react-query';

import * as rumApi from '@/api/rum';

import type { TraceTree } from '../model/tree';

/**
 * Where a span keeps the RUM session it served. OpenTelemetry's browser and
 * mobile instrumentation names it `session.id`; MoleSignal's RUM events keep
 * it as `session_id`, and a service that copies it from them names it so.
 */
export const SESSION_KEYS = ['session.id', 'session_id', 'rum.session_id', 'browser.session.id'] as const;

/** How long a RUM session can last: the browser SDK ends one after four hours. */
const SESSION_MICROS = 4 * 60 * 60 * 1_000_000;
/** A minute either side of the trace: the action that made its request is stamped when the request ended. */
const ACTION_PADDING_MICROS = 60 * 1_000_000;

/** The user session a trace served, where one of its spans says. */
export function sessionOfSpans(tree: TraceTree): string | undefined {
  for (const node of tree.nodes) {
    for (const attributes of [node.span.attributes, node.span.resource?.attributes]) {
      for (const key of SESSION_KEYS) {
        const value = attributes?.[key];
        if (typeof value === 'string' && value.trim()) return value.trim();
      }
    }
  }

  return undefined;
}

/**
 * The RUM session a trace served: the one its spans name, or else the one
 * whose action's request carried the trace's id, as the browser SDK sends it
 * in `traceparent`. Only a trace whose root was called from outside it, by a
 * parent span it does not hold, can have come from a browser: the others are
 * not looked for. `undefined` for a trace no session made.
 */
export function useTraceSession(tree: TraceTree): string | undefined {
  const named = sessionOfSpans(tree);
  const from = Math.floor(tree.startNs / 1_000) - ACTION_PADDING_MICROS;
  const to = Math.ceil(tree.endNs / 1_000) + ACTION_PADDING_MICROS;

  const found = useQuery({
    queryKey: ['trace', 'rum-session', tree.traceId, from, to],
    queryFn: () => rumApi.traceSession(tree.traceId, { from, to }),
    enabled: named === undefined && tree.root.orphan,
    staleTime: 60_000,
    retry: false,
  });

  return named ?? found.data?.session?.sessionId ?? undefined;
}

/**
 * The RUM page of a session, over the stretch it can have run in around the
 * trace: the page reads the session in the time it is given.
 */
export function rumSessionHref(sessionId: string, tree: TraceTree, nowMicros = Date.now() * 1_000): string {
  const from = new Date(Math.floor((tree.startNs / 1_000 - SESSION_MICROS) / 1_000)).toISOString();
  const to = new Date(Math.floor(Math.min(nowMicros, tree.endNs / 1_000 + SESSION_MICROS) / 1_000)).toISOString();
  const params = new URLSearchParams({ from, to, time: `${from}..${to}` });

  return `/rum/sessions/view/${encodeURIComponent(sessionId)}?${params}`;
}
