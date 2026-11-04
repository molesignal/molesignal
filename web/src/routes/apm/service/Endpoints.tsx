import { ArrowDown, ArrowUp, Search, Star, Timer } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ServiceDetailResponse, TransactionSummary } from '@/api/apm';
import { DEFAULT_EXPLORER_PAGE_SIZE, explorerPageSizeIn, explorerPageSizesFor } from '@/pagination/explorerPageSize';
import { cn } from '@/shell/lib/cn';
import { LONG_LIST_ROW_CLASS, longListRowStyle } from '@/shell/list/longList';
import { ResultPagination } from '@/shell/ResultPagination';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { throughputOf } from '../catalog/model';
import { formatCount, formatDuration, formatRate, formatThroughput } from '../format';
import { transactionKey, useKeyTransactions } from './keyTransactions';
import { EndpointKind, EndpointName } from './Overview';
import type { ServiceAddress } from './useService';
import { ApmColumnMenu } from '../frame/ColumnMenu';
import { FilterToggle } from '../frame/FilterToggle';
import { gridMinWidth } from '../frame/gridWidth';
import { OpenRow, StarToggle } from '../frame/OpenRow';
import { ApmCard } from '../frame/Page';
import { useApmColumns } from '../frame/useApmColumns';
import { useAddressedDrawer } from '../frame/useApmScope';
import { useApmSettings } from '../settings/useApmSettings';

const COLUMNS = ['requests', 'throughput', 'error_rate', 'p50', 'p90', 'p95', 'p99', 'average', 'apdex', 'total_time'] as const;
type EndpointColumn = (typeof COLUMNS)[number];
const DEFAULT_COLUMNS: readonly EndpointColumn[] = ['requests', 'error_rate', 'p50', 'p95', 'p99', 'total_time'];
const WIDTH: Record<EndpointColumn, string> = {
  requests: '88px',
  throughput: '96px',
  error_rate: '80px',
  p50: '80px',
  p90: '80px',
  p95: '80px',
  p99: '80px',
  average: '80px',
  apdex: '64px',
  total_time: '160px',
};
const KINDS = ['all', 'http', 'rpc', 'messaging', 'span'] as const;
const ROW_STYLE = { height: 32, ...longListRowStyle(31) };

type SortKey = 'kind' | 'endpoint' | EndpointColumn;

function valueOf(item: TransactionSummary, key: SortKey): string | number {
  switch (key) {
    case 'kind':
      return item.transaction.kind;
    case 'endpoint':
      return item.transaction.name.toLocaleLowerCase();
    case 'requests':
    case 'throughput':
      return item.red.requestCount;
    case 'error_rate':
      return item.red.errorRate;
    case 'p50':
      return item.red.p50Time ?? -1;
    case 'p90':
      return item.red.p90Time ?? -1;
    case 'p95':
      return item.red.p95Time ?? -1;
    case 'p99':
      return item.red.p99Time ?? -1;
    case 'average':
      return item.red.durationAverage ?? -1;
    case 'apdex':
      return item.red.apdex ?? -1;
    case 'total_time':
      return item.totalTime;
  }
}

/**
 * The service's endpoints, one line each as dense as the trace table: what
 * each served, how often it failed, how long it took, and its share of the
 * service's time, paged with the sizes of the Traces page. A star marks the
 * key transactions, and the list can keep only them. A line opens the
 * endpoint in a drawer.
 */
export function ServiceEndpoints({ address, detail }: { address: ServiceAddress; detail: ServiceDetailResponse }) {
  const { t } = useTranslation('apm');
  const drawer = useAddressedDrawer('endpoint');
  const columns = useApmColumns('endpoints', COLUMNS, DEFAULT_COLUMNS);
  const keyed = useKeyTransactions(address);
  const [search, setSearch] = React.useState('');
  const [kind, setKind] = React.useState<(typeof KINDS)[number]>('all');
  const [keyOnly, setKeyOnly] = React.useState(false);
  const [longOnly, setLongOnly] = React.useState(false);
  const longTaskMicros = useApmSettings().longTaskTime;
  const [sort, setSort] = React.useState<{
    key: SortKey;
    direction: 'asc' | 'desc';
  }>({ key: 'total_time', direction: 'desc' });
  const [page, setPage] = React.useState(1);
  const [chosenSize, setChosenSize] = React.useState(DEFAULT_EXPLORER_PAGE_SIZE);
  const range = {
    from: new Date(detail.meta.range.from / 1_000),
    to: new Date(detail.meta.range.to / 1_000),
  };
  const totalTime = detail.transactions.reduce((sum, item) => sum + item.totalTime, 0);
  const tracks = ['24px', '56px', 'minmax(240px,1fr)', ...columns.visible.map((column) => WIDTH[column])];
  const grid = tracks.join(' ');
  const minWidth = gridMinWidth(tracks);

  const rows = React.useMemo(() => {
    const needle = search.trim().toLocaleLowerCase();
    const sign = sort.direction === 'asc' ? 1 : -1;

    return detail.transactions
      .filter(
        (item) =>
          (kind === 'all' || item.transaction.kind === kind) &&
          (!needle || item.transaction.name.toLocaleLowerCase().includes(needle)) &&
          (!keyOnly || keyed.keys.has(transactionKey(item.transaction))) &&
          (!longOnly || isLongTask(item, longTaskMicros)),
      )
      .sort((a, b) => {
        const left = valueOf(a, sort.key);
        const right = valueOf(b, sort.key);
        return left < right ? -sign : left > right ? sign : a.transaction.name.localeCompare(b.transaction.name);
      });
  }, [detail.transactions, keyOnly, keyed.keys, kind, longOnly, longTaskMicros, search, sort]);

  React.useEffect(() => setPage(1), [detail.transactions, keyOnly, kind, longOnly, search, sort]);
  const longTasks = detail.transactions.filter((item) => isLongTask(item, longTaskMicros)).length;

  const pageSizes = explorerPageSizesFor(rows.length);
  const pageSize = explorerPageSizeIn(pageSizes, chosenSize);
  const pageCount = Math.max(1, Math.ceil(rows.length / pageSize));
  const shown = rows.slice((Math.min(page, pageCount) - 1) * pageSize, Math.min(page, pageCount) * pageSize);

  const header = (key: SortKey) => {
    const active = sort.key === key;
    const numeric = key !== 'endpoint' && key !== 'kind';

    return (
      <span key={key} role="columnheader" className={cn('min-w-0', numeric && 'text-right')}>
        <button
          type="button"
          onClick={() => setSort(active ? { key, direction: sort.direction === 'asc' ? 'desc' : 'asc' } : { key, direction: numeric ? 'desc' : 'asc' })}
          className={cn('inline-flex max-w-full items-center gap-1 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none', active && 'text-tx-0')}
        >
          <span className="truncate">{t(`endpoints.columns.${key}`)}</span>
          {active && (sort.direction === 'desc' ? <ArrowDown aria-hidden className="h-3 w-3" /> : <ArrowUp aria-hidden className="h-3 w-3" />)}
        </button>
      </span>
    );
  };

  return (
    <ApmCard
      testId="endpoints"
      title={
        <span className="flex items-center gap-2">
          <label className="relative">
            <span className="sr-only">{t('endpoints.search')}</span>
            <Search aria-hidden className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3" />
            <input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t('endpoints.search')}
              className="h-8 w-60 rounded-md border-0 bg-[var(--control-surface)] pl-8 pr-2.5 font-sans text-xs font-body text-tx-1 outline-none placeholder:text-tx-3 hover:bg-bg-3 focus-visible:bg-bg-3"
            />
          </label>
          <FittedSelect
            aria-label={t('endpoints.kind_filter')}
            value={kind}
            onValueChange={(value) => setKind(value as (typeof KINDS)[number])}
            options={KINDS.map((value) => ({
              value,
              label: t(`endpoints.kinds.${value}`),
            }))}
            maxWidth={160}
            className="h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3"
          />
          {(keyed.keys.size > 0 || keyOnly) && (
            <FilterToggle
              pressed={keyOnly}
              onToggle={() => setKeyOnly((current) => !current)}
              icon={<Star aria-hidden className="h-3.5 w-3.5 text-[var(--yellow-ink)]" fill={keyOnly ? 'currentColor' : 'none'} />}
              data-apm-endpoints-key-only=""
            >
              {t('endpoints.key_only', { count: keyed.keys.size })}
            </FilterToggle>
          )}
          {(longTasks > 0 || longOnly) && (
            <span title={t('endpoints.long_tasks_hint', { threshold: formatDuration(longTaskMicros) })}>
              <FilterToggle
                pressed={longOnly}
                onToggle={() => setLongOnly((current) => !current)}
                icon={<Timer aria-hidden className="h-3.5 w-3.5 text-[var(--orange-ink)]" />}
                data-apm-endpoints-long-tasks=""
              >
                {t('endpoints.long_tasks', { count: longTasks })}
              </FilterToggle>
            </span>
          )}
          <span className="font-sans text-xs text-tx-3">{t('endpoints.count', { count: rows.length })}</span>
        </span>
      }
      actions={<ApmColumnMenu columns={columns} label={(id) => t(`endpoints.columns.${id}`)} />}
    >
      {rows.length === 0 ? (
        <p className="px-3 py-8 text-center font-sans text-xs text-tx-3">{t('endpoints.empty')}</p>
      ) : (
        <div role="table" aria-label={t('endpoints.table')} className="overflow-x-auto">
          <div
            role="row"
            className="sticky top-0 grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0"
            style={{ gridTemplateColumns: grid, minWidth }}
          >
            {/* The star's column: named for screen readers, and holding its place in the grid. */}
            <span role="columnheader">
              <span className="sr-only">{t('endpoints.key')}</span>
            </span>
            {header('kind')}
            {header('endpoint')}
            {columns.visible.map((column) => header(column))}
          </div>
          {shown.map((item) => (
            <OpenRow
              key={`${item.transaction.kind}:${item.transaction.name}:${item.version ?? ''}`}
              data-apm-endpoint-row={item.transaction.name}
              onOpen={() =>
                drawer.open(item.transaction.name, {
                  kind: item.transaction.kind,
                })
              }
              style={{ ...ROW_STYLE, gridTemplateColumns: grid, minWidth }}
              className={cn(
                'grid w-full cursor-pointer items-center gap-3 border-b border-[var(--row-rule)] px-3 text-left font-sans text-xs outline-none hover:bg-bg-2 focus-visible:bg-bg-2',
                LONG_LIST_ROW_CLASS,
              )}
            >
              <span role="cell" className="-ml-1 flex items-center">
                <StarToggle
                  on={keyed.isKey(item.transaction)}
                  label={keyed.title(item.transaction)}
                  onToggle={() => keyed.toggle(item.transaction)}
                  disabled={!keyed.canChange}
                  className={cn(!keyed.canChange && !keyed.isKey(item.transaction) && 'invisible')}
                />
              </span>
              <EndpointKind item={item} />
              <EndpointName item={item} />
              {columns.visible.map((column) => (
                <span key={column} role="cell" className="flex min-w-0 items-center justify-end gap-2 text-right tabular-nums">
                  {column === 'requests' && <span className="text-tx-0">{formatCount(item.red.requestCount)}</span>}
                  {column === 'throughput' && <span className="text-tx-0">{formatThroughput(throughputOf(item.red.requestCount, range))}</span>}
                  {column === 'error_rate' && (
                    <span className={item.red.errorCount > 0 ? 'font-strong text-[var(--red-ink)]' : 'text-tx-0'}>{formatRate(item.red.errorRate)}</span>
                  )}
                  {(column === 'p50' || column === 'p90' || column === 'p95' || column === 'p99') && (
                    <span className="text-tx-0">{formatDuration(item.red[`${column}Time`] ?? undefined)}</span>
                  )}
                  {column === 'average' && <span className="text-tx-0">{formatDuration(item.red.durationAverage)}</span>}
                  {column === 'apdex' && <span className="text-tx-0">{item.red.apdex == null ? '—' : item.red.apdex.toFixed(2)}</span>}
                  {column === 'total_time' && <TimeShare share={totalTime > 0 ? item.totalTime / totalTime : 0} />}
                </span>
              ))}
            </OpenRow>
          ))}
        </div>
      )}
      {rows.length > 0 && (
        <EndpointPages
          page={Math.min(page, pageCount)}
          pageCount={pageCount}
          pageSize={pageSize}
          pageSizes={pageSizes}
          onPage={setPage}
          onPageSize={(size) => {
            setChosenSize(size);
            setPage(1);
          }}
        />
      )}
    </ApmCard>
  );
}

function TimeShare({ share }: { share: number }) {
  return (
    <>
      <span aria-hidden className="relative h-1.5 w-20 overflow-hidden rounded-full bg-bg-3">
        <span className="absolute inset-y-0 left-0 rounded-full bg-[var(--chart-1)]" style={{ width: `${Math.max(2, share * 100)}%` }} />
      </span>
      <span className="w-12 text-tx-0">{formatRate(share)}</span>
    </>
  );
}

function EndpointPages({
  page,
  pageCount,
  pageSize,
  pageSizes,
  onPage,
  onPageSize,
}: {
  page: number;
  pageCount: number;
  pageSize: number;
  pageSizes: number[];
  onPage: (page: number) => void;
  onPageSize: (size: number) => void;
}) {
  const { t } = useTranslation('apm');

  return (
    <ResultPagination
      page={page}
      pageCount={pageCount}
      pageSize={pageSize}
      pageSizeOptions={pageSizes}
      pageLabel={t('pagination.page_summary', { page, pages: pageCount })}
      ariaLabel={t('pagination.aria')}
      pageSizeAriaLabel={t('pagination.page_size_aria')}
      firstAriaLabel={t('pagination.first_aria')}
      previousAriaLabel={t('pagination.previous_aria')}
      nextAriaLabel={t('pagination.next_aria')}
      lastAriaLabel={t('pagination.last_aria')}
      onPageChange={onPage}
      onPageSizeChange={onPageSize}
      className="border-t-0 bg-transparent"
    />
  );
}

/** An endpoint whose slowest requests (P99) take at least `threshold`, the organization's long task. */
export function isLongTask(item: TransactionSummary, threshold: number): boolean {
  return (item.red.p99Time ?? 0) >= threshold;
}
