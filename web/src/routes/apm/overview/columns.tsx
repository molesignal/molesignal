import type { DataTableColumn } from '@/admin';
import type {
  DependencySummary,
  ErrorSummary,
  ServiceSummary,
  TransactionSummary,
} from '@/api/apm';
import { cn } from '@/shell/lib/cn';

import { HealthDot } from '../components';
import { formatCount, formatDuration, formatRate } from '../format';

type Translate = (key: string) => string;

/**
 * A figure with its size drawn beside it, so a column can be compared down the
 * page without reading every number. The bar is the share of the largest value
 * in the column; the figure stays the precise reading.
 */
function Meter({
  value,
  ratio,
  barClassName = 'bg-indigo/55',
}: {
  value: string;
  ratio: number;
  barClassName?: string;
}) {
  const share = Number.isFinite(ratio) ? Math.min(1, Math.max(0, ratio)) : 0;
  return (
    <span data-meter className="flex items-center gap-[10px]">
      <span className="w-[64px] shrink-0 tabular-nums">{value}</span>
      <span aria-hidden="true" className="h-[4px] w-full max-w-[140px] overflow-hidden rounded-full bg-bd-0">
        <span
          className={cn('block h-full rounded-full', barClassName)}
          style={{ width: `${share * 100}%` }}
        />
      </span>
    </span>
  );
}

function largest(values: number[]): number {
  return Math.max(0, ...values);
}

function errorRateClass(rate: number): string {
  if (rate >= 0.1) return 'bg-red/70';
  if (rate >= 0.02) return 'bg-yellow/70';
  return 'bg-green/55';
}

export function serviceColumns(
  t: Translate,
  rows: readonly ServiceSummary[],
): DataTableColumn<ServiceSummary>[] {
  const maxRequests = largest(rows.map((row) => row.red.request_count));
  const maxErrorRate = largest(rows.map((row) => row.red.error_rate));
  const maxP95 = largest(rows.map((row) => row.red.p95_micros ?? 0));
  return [
    {
      key: 'service',
      header: t('columns.service'),
      cell: (row) => (
        <span className="inline-flex items-center gap-2">
          <HealthDot status={row.health} />
          <span>
            <span className="block font-strong text-tx-0">{row.service.name}</span>
            <span className="text-xs text-tx-3">
              {row.service.namespace} · {row.service.environment}
            </span>
          </span>
        </span>
      ),
    },
    {
      key: 'requests',
      header: t('columns.requests'),
      cell: (row) => (
        <Meter
          value={formatCount(row.red.request_count)}
          ratio={row.red.request_count / maxRequests}
        />
      ),
    },
    {
      key: 'errors',
      header: t('columns.error_rate'),
      cell: (row) => (
        <Meter
          value={formatRate(row.red.error_rate)}
          ratio={row.red.error_rate / maxErrorRate}
          barClassName={errorRateClass(row.red.error_rate)}
        />
      ),
    },
    {
      key: 'p95',
      header: t('columns.p95'),
      cell: (row) => (
        <Meter
          value={formatDuration(row.red.p95_micros)}
          ratio={(row.red.p95_micros ?? 0) / maxP95}
          barClassName="bg-orange/55"
        />
      ),
    },
  ];
}

export function transactionColumns(t: Translate): DataTableColumn<TransactionSummary>[] {
  return [
    {
      key: 'transaction',
      header: t('columns.transaction'),
      cell: (row) => (
        <span>
          <span className="block font-strong text-tx-0">{row.transaction.name}</span>
          <span className="text-xs text-tx-3">
            {t(`transaction_kinds.${row.transaction.kind}`)}
          </span>
        </span>
      ),
    },
    {
      key: 'requests',
      header: t('columns.requests'),
      cell: (row) => formatCount(row.red.request_count),
    },
    {
      key: 'errors',
      header: t('columns.error_rate'),
      cell: (row) => formatRate(row.red.error_rate),
    },
    {
      key: 'p95',
      header: t('columns.p95'),
      cell: (row) => formatDuration(row.red.p95_micros),
    },
    {
      key: 'total',
      header: t('columns.total_time'),
      cell: (row) => formatDuration(row.total_time_micros),
    },
  ];
}

export function dependencyColumns(t: Translate): DataTableColumn<DependencySummary>[] {
  return [
    { key: 'target', header: t('columns.target'), cell: (row) => row.dependency.target },
    {
      key: 'category',
      header: t('columns.category'),
      cell: (row) => t(`dependency_categories.${row.dependency.category}`),
    },
    { key: 'p95', header: t('columns.p95'), cell: (row) => formatDuration(row.red.p95_micros) },
  ];
}

export function errorColumns(t: Translate): DataTableColumn<ErrorSummary>[] {
  return [
    { key: 'error', header: t('columns.error'), cell: (row) => row.error.error_type },
    { key: 'service', header: t('columns.service'), cell: (row) => row.service.name },
    {
      key: 'count',
      header: t('columns.occurrences'),
      cell: (row) => formatCount(row.occurrence_count),
    },
  ];
}
