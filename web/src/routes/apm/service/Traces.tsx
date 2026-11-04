import { useQuery } from '@tanstack/react-query';
import { ExternalLink, Waypoints } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { traces as listTraces, type TraceFilter, type TraceListSort } from '@/api/web';
import { useCursorPagination } from '@/pagination/useCursorPagination';
import { listedTraceWindow } from '@/routes/traces/explorer/drawer/model';
import { displayTrace } from '@/routes/traces/explorer/list/useTraceList';
import { DEFAULT_TRACE_COLUMN_IDS, traceTableColumns } from '@/routes/traces/explorer/table/columns';
import { TraceTable } from '@/routes/traces/explorer/table/TraceTable';
import { DEFAULT_TRACE_PAGE_SIZE, TracePagination } from '@/routes/traces/Pagination';

import { tracesHref } from '../endpointFilters';
import type { ServiceAddress } from './useService';
import { ApmCard, ApmQueryState } from '../frame/Page';
import { useTraceOpener } from '../frame/TraceDrawerHost';

const COLUMNS = traceTableColumns(DEFAULT_TRACE_COLUMN_IDS, new Set());

/**
 * The service's traces without leaving APM: the trace table of the Traces
 * page, narrowed to traces the service took part in (and its version, when
 * one is chosen), paged and sized as the Traces page pages them. A trace
 * opens in the same drawer; the full explorer is a link away with the same
 * query.
 */
export function ServiceTraces({ address }: { address: ServiceAddress }) {
  const { t } = useTranslation('apm');
  const { openTrace } = useTraceOpener();
  const [sort, setSort] = React.useState<TraceListSort>('latest');
  const from = address.range.from.getTime() * 1_000;
  const to = address.range.to.getTime() * 1_000;
  const filters: TraceFilter[] = [
    { field: 'service.name', op: '=', value: address.name },
    ...(address.scope.version
      ? [
          {
            field: 'service.version',
            op: '=' as const,
            value: address.scope.version,
          },
        ]
      : []),
  ];
  const pages = useCursorPagination({
    contextKey: JSON.stringify([address.orgId, address.name, address.scope.version, from, to, sort]),
    defaultPageSize: DEFAULT_TRACE_PAGE_SIZE,
  });

  const query = useQuery({
    queryKey: ['apm', 'service-traces', address.orgId, address.name, address.scope.version, from, to, sort, pages.cursor ?? '', pages.pageSize],
    queryFn: () =>
      listTraces({
        from,
        to,
        filters,
        sort,
        limit: pages.pageSize,
        ...(pages.cursor ? { cursor: pages.cursor } : {}),
      }),
    enabled: Boolean(address.orgId),
  });

  const rows = React.useMemo(() => (query.data?.items ?? []).map(displayTrace), [query.data]);

  return (
    <ApmCard
      icon={Waypoints}
      tone="text-[var(--chart-1)]"
      testId="service-traces"
      title={t('traces.title')}
      description={t('traces.description')}
      actions={
        <Link
          to={tracesHref(filters, { from, to })}
          className="inline-flex items-center gap-1 font-sans text-xs font-strong text-tx-2 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none"
        >
          {t('traces.open_explorer')}
          <ExternalLink aria-hidden className="h-3 w-3" />
        </Link>
      }
      bodyClassName="flex min-h-[360px] flex-col"
    >
      <ApmQueryState query={query} empty={rows.length === 0} emptyTitle={t('traces.empty')}>
        <TraceTable
          traces={rows}
          columns={COLUMNS}
          sort={sort}
          sortEnabled
          onSortChange={setSort}
          openId={null}
          onOpen={(trace, row) =>
            openTrace(
              {
                traceId: trace.id,
                window: listedTraceWindow(trace.startNs, trace.durationMs),
              },
              row,
            )
          }
        />
        <TracePagination
          model={{
            kind: 'cursor',
            pageSize: pages.pageSize,
            hasPrevious: Boolean(query.data?.previousCursor),
            hasNext: Boolean(query.data?.nextCursor),
            pending: query.isFetching,
            onPrevious: () => pages.goPrevious(query.data),
            onNext: () => pages.goNext(query.data),
            onPageSizeChange: pages.setPageSize,
          }}
        />
      </ApmQueryState>
    </ApmCard>
  );
}
