import { useQuery } from '@tanstack/react-query';
import { ArrowDownLeft, ArrowUpRight } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { linkedSpans, type LinkedSpan } from '@/api/traceExplorer';
import type { Span, TraceWindow } from '@/api/web';
import { cn } from '@/shell/lib/cn';

import { formatTraceDurationNs } from '../duration';
import { attributeText } from '../model/span';

/** How far around the trace the spans linking to it are looked for. */
export const LINK_SEARCH_PADDING_MICROS = 60 * 60 * 1_000_000;

/** A trace to open from a link, at the span the link names. */
export type OpenLinkedTrace = (traceId: string, spanId: string | null, aroundNs: number | null) => void;

/**
 * The span's links to spans outside its own line of calls: the ones it keeps
 * itself, to what it followed from, and the spans of other traces that link
 * to it, found around the trace's time. Each opens its trace at its span.
 */
export function SpanLinks({
  traceId,
  span,
  window,
  onOpenTrace,
}: {
  traceId: string;
  span: Span;
  /** When the trace ran, widened: where the spans linking to it are looked for. */
  window: TraceWindow;
  onOpenTrace: OpenLinkedTrace;
}) {
  const { t } = useTranslation('traces');
  const incoming = useLinkedSpans(traceId, window);
  const toThisSpan = (incoming.data?.spans ?? []).filter((linked) => linked.linkedSpanId === span.spanId);

  return (
    <div className="space-y-3">
      <section aria-label={t('view.links.outgoing')}>
        <h3 className="flex items-center gap-1.5 px-[6px] pb-1 font-sans text-xs font-strong text-tx-3">
          <ArrowUpRight aria-hidden="true" className="h-3.5 w-3.5" />
          {t('view.links.outgoing')}
          <span className="font-body tabular-nums">{span.links.length}</span>
        </h3>
        {span.links.length === 0 ? (
          <p className="px-[6px] font-sans text-xs text-tx-3">{t('view.links.none_outgoing')}</p>
        ) : (
          <ul className="space-y-1">
            {span.links.map((link, index) => (
              <li key={`${link.trace_id}-${link.span_id}-${index}`}>
                <LinkLine
                  traceId={link.trace_id}
                  spanId={link.span_id}
                  attributes={link.attributes}
                  sameTrace={link.trace_id === traceId}
                  onOpen={() => onOpenTrace(link.trace_id, link.span_id || null, null)}
                />
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-label={t('view.links.incoming')}>
        <h3 className="flex items-center gap-1.5 px-[6px] pb-1 font-sans text-xs font-strong text-tx-3">
          <ArrowDownLeft aria-hidden="true" className="h-3.5 w-3.5" />
          {t('view.links.incoming')}
          {incoming.data && <span className="font-body tabular-nums">{toThisSpan.length}</span>}
        </h3>
        <IncomingState state={incoming} count={toThisSpan.length} />
        {toThisSpan.length > 0 && (
          <ul className="space-y-1">
            {toThisSpan.map((linked) => (
              <li key={`${linked.traceId}-${linked.spanId}`}>
                <IncomingLine linked={linked} onOpen={() => onOpenTrace(linked.traceId, linked.spanId, linked.startNs)} />
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

/** The spans of other traces that link into this one: read once per trace, when first asked for. */
export function useLinkedSpans(traceId: string, window: TraceWindow, enabled = true) {
  return useQuery({
    queryKey: ['web', 'trace-linked', traceId, window.from, window.to],
    queryFn: () => linkedSpans(traceId, window),
    enabled,
    staleTime: 60_000,
  });
}

function IncomingState({ state, count }: { state: ReturnType<typeof useLinkedSpans>; count: number }) {
  const { t } = useTranslation('traces');

  if (state.isLoading) return <p className="px-[6px] font-sans text-xs text-tx-3">{t('view.links.searching')}</p>;
  if (state.isError) return <p className="px-[6px] font-sans text-xs text-[var(--red-ink)]">{t('view.links.failed')}</p>;
  if (state.data?.unavailable) {
    return <p className="px-[6px] font-sans text-xs text-tx-3">{t(`view.links.unavailable.${state.data.unavailable}`)}</p>;
  }
  if (count === 0) return <p className="px-[6px] font-sans text-xs text-tx-3">{t('view.links.none_incoming')}</p>;
  if (state.data?.more) return <p className="px-[6px] pb-1 font-sans text-xs text-tx-3">{t('view.links.more')}</p>;
  return null;
}

function LinkLine({
  traceId,
  spanId,
  attributes,
  sameTrace,
  onOpen,
}: {
  traceId: string;
  spanId: string;
  attributes: Record<string, unknown>;
  sameTrace: boolean;
  onOpen: () => void;
}) {
  const { t } = useTranslation('traces');
  const tags = Object.entries(attributes ?? {});

  return (
    <button
      type="button"
      data-testid="trace-span-link"
      onClick={onOpen}
      className="w-full rounded-md bg-bg-1 px-[10px] py-[7px] text-left hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none"
    >
      <span className="flex min-w-0 items-center gap-1.5 font-sans text-xs">
        <span className="shrink-0 text-tx-3">{t(sameTrace ? 'view.links.this_trace' : 'view.links.trace')}</span>
        <span className="min-w-0 truncate tabular-nums text-tx-0">{traceId}</span>
      </span>
      <span className="mt-0.5 flex min-w-0 items-center gap-1.5 font-sans text-xs">
        <span className="shrink-0 text-tx-3">{t('view.links.span')}</span>
        <span className="min-w-0 truncate tabular-nums text-tx-1">{spanId || '—'}</span>
      </span>
      {tags.length > 0 && (
        <span className="mt-1 flex flex-wrap gap-1">
          {tags.map(([key, value]) => (
            <span key={key} className="max-w-full truncate rounded-sm bg-bg-2 px-1.5 font-sans text-xs text-tx-2">
              {key}={attributeText(value)}
            </span>
          ))}
        </span>
      )}
    </button>
  );
}

function IncomingLine({ linked, onOpen }: { linked: LinkedSpan; onOpen: () => void }) {
  const failed = linked.status === 'ERROR';

  return (
    <button
      type="button"
      data-testid="trace-span-linked"
      onClick={onOpen}
      className="w-full rounded-md bg-bg-1 px-[10px] py-[7px] text-left hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none"
    >
      <span className="flex min-w-0 items-center gap-1.5 font-sans text-xs">
        <span className="min-w-0 truncate font-strong text-tx-0">{linked.service}</span>
        <span className="min-w-0 flex-1 truncate text-tx-1">{linked.operation}</span>
        <span className={cn('shrink-0 tabular-nums', failed ? 'text-[var(--red-ink)]' : 'text-tx-2')}>
          {formatTraceDurationNs(linked.durationNs)}
        </span>
      </span>
      <span className="mt-0.5 block truncate font-sans text-xs tabular-nums text-tx-3">{linked.traceId}</span>
    </button>
  );
}
