import { useQuery } from '@tanstack/react-query';
import { ExternalLink, Hourglass } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { TransactionIdentity } from '@/api/apm';
import { traceSpans } from '@/api/traceExplorer';
import type { TraceFilter } from '@/api/web';
import { useTimeFormatter } from '@/lib/time';
import { listedTraceWindow } from '@/routes/traces/explorer/drawer/model';
import { cn } from '@/shell/lib/cn';
import { formatTraceDurationNs } from '@/viz/trace/duration';
import { OUTCOME_FILL, requestOutcome } from '@/viz/trace/model/span';

import type { ServiceAddress } from './useService';
import { endpointFilterCandidates, isUnknownTraceField, tracesHref } from '../endpointFilters';
import { ApmCard, ApmQueryState } from '../frame/Page';
import { useTraceOpener } from '../frame/TraceDrawerHost';

/**
 * The endpoint's slowest requests from the trace store, and the filters that
 * found them. Each candidate spelling of the fields the endpoint is named
 * from is tried in turn; the first that finds spans holds. A spelling naming
 * a field the spans do not have finds nothing and is passed over. The
 * time-spent card reads the same filters.
 */
export function useSlowRequests(address: ServiceAddress, transaction: TransactionIdentity) {
  const candidates = endpointFilterCandidates(address.name, transaction);
  const from = address.range.from.getTime() * 1_000;
  const to = address.range.to.getTime() * 1_000;

  const query = useQuery({
    queryKey: ['apm', 'slow-requests', address.orgId, address.name, transaction.kind, transaction.name, from, to],
    enabled: candidates.length > 0,
    queryFn: async () => {
      let known: TraceFilter[] | null = null;
      for (const filters of candidates) {
        try {
          const page = await traceSpans({
            from,
            to,
            filters,
            sort: 'duration_desc',
            limit: 15,
          });
          if (page.items.length > 0) return { filters, items: page.items };
          known ??= filters;
        } catch (error) {
          if (!isUnknownTraceField(error)) throw error;
        }
      }
      return { filters: known ?? [], items: [] };
    },
  });

  return { candidates, from, to, query };
}

/**
 * The slowest requests of the endpoint in the time in view, each opening its
 * trace; the link to Traces carries the filters that found them.
 */
export function SlowRequests({ address, transaction }: { address: ServiceAddress; transaction: TransactionIdentity }) {
  const { t } = useTranslation('apm');
  const time = useTimeFormatter();
  const { openTrace } = useTraceOpener();
  const { candidates, from, to, query } = useSlowRequests(address, transaction);

  return (
    <ApmCard
      icon={Hourglass}
      tone="text-[var(--orange-ink)]"
      title={t('endpoint.slow_requests')}
      description={t('endpoint.slow_requests_description')}
      testId="slow-requests"
      actions={
        query.data && query.data.filters.length > 0 ? (
          <Link
            to={tracesHref(query.data.filters, { from, to }, 'spans')}
            className="inline-flex items-center gap-1 font-sans text-xs font-strong text-tx-2 hover:text-tx-0"
          >
            {t('endpoint.in_traces')}
            <ExternalLink aria-hidden className="h-3 w-3" />
          </Link>
        ) : undefined
      }
    >
      {candidates.length === 0 ? (
        <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('endpoint.other_has_no_requests')}</p>
      ) : (
        <ApmQueryState query={query} empty={(query.data?.items.length ?? 0) === 0} emptyTitle={t('endpoint.no_requests')} compact>
          {query.data?.items.map((span) => (
            <button
              key={`${span.traceId}:${span.spanId}`}
              type="button"
              data-apm-slow-request={span.traceId}
              onClick={(event) =>
                openTrace(
                  {
                    traceId: span.traceId,
                    spanId: span.spanId,
                    window: listedTraceWindow(span.startNs, span.durationNs / 1_000_000),
                  },
                  event.currentTarget,
                )
              }
              className="grid h-8 w-full grid-cols-[minmax(0,1fr)_88px_72px] items-center gap-3 border-b border-bd-0 px-3 text-left font-sans text-xs outline-none last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2"
            >
              <span className="flex min-w-0 items-center gap-2">
                <span aria-hidden className={cn('h-2 w-2 shrink-0 rounded-full', OUTCOME_FILL[requestOutcome(span.status === 'ERROR', span.httpStatusCode)])} />
                <span className="truncate tabular-nums text-tx-2">{time.micros(Math.floor(span.startNs / 1_000))}</span>
              </span>
              <span className="truncate tabular-nums text-tx-3" title={span.traceId}>
                {span.traceId.slice(0, 8)}
              </span>
              <span className="text-right font-strong tabular-nums text-tx-0">{formatTraceDurationNs(span.durationNs)}</span>
            </button>
          ))}
        </ApmQueryState>
      )}
    </ApmCard>
  );
}
