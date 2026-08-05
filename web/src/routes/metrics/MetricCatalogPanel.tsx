import { Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MetricCatalogEntry } from '@/api/metricsCatalog';
import { resolveMetricType } from '@/lib/metricTypes';
import { CursorPagination } from '@/shell/CursorPagination';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';

import { MetricTypePrefix } from './MetricTypePrefix';

const PAGE_SIZE_OPTIONS = [20, 50, 100];

interface MetricCatalogPanelProps {
  metrics: MetricCatalogEntry[];
  pending: boolean;
  error: unknown;
  selectedMetricName: string | null;
  filter: string;
  open: boolean;
  pageSize: number;
  hasPrevious: boolean;
  hasNext: boolean;
  onFilterChange: (filter: string) => void;
  onOpenChange: (open: boolean) => void;
  onPickMetric: (metric: MetricCatalogEntry) => void;
  onPrevious: () => void;
  onNext: () => void;
  onPageSizeChange: (pageSize: number) => void;
}

export function MetricCatalogPanel({
  metrics,
  pending,
  error,
  selectedMetricName,
  filter,
  open,
  pageSize,
  hasPrevious,
  hasNext,
  onFilterChange,
  onOpenChange,
  onPickMetric,
  onPrevious,
  onNext,
  onPageSizeChange,
}: MetricCatalogPanelProps) {
  const { t } = useTranslation('metrics');

  const changeFilter = React.useCallback(
    (nextFilter: string) => {
      onFilterChange(nextFilter);
    },
    [onFilterChange],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="flex h-[min(78vh,720px)] w-[calc(100vw-2rem)] max-w-[960px] flex-col gap-0 overflow-hidden p-0"
        data-testid="metrics-browser-dialog"
      >
        <DialogHeader className="border-b border-bd-0 bg-bg-2/70 px-5 py-4 pr-12">
          <DialogTitle>{t('explore.catalog.browser_title')}</DialogTitle>
          <DialogDescription>
            {t('explore.catalog.search_description')}
          </DialogDescription>
        </DialogHeader>

        <div className="border-b border-bd-0 p-4">
          <div className="flex h-11 items-center gap-2 rounded-md border border-bd-1 bg-bg-1 px-3 font-sans text-xs">
            <Search className="h-3.5 w-3.5 text-tx-3" aria-hidden="true" />
            <input
              autoFocus
              value={filter}
              onChange={(event) => changeFilter(event.target.value)}
              placeholder={t('explore.catalog.filter_placeholder')}
              aria-label={t('explore.catalog.search_aria')}
              className="min-w-0 flex-1 bg-transparent text-base text-tx-0 placeholder:text-tx-3 focus:outline-none sm:text-sm"
            />
            <span className="type-micro shrink-0 tabular-nums text-tx-3">
              {t('explore.catalog.result_count', { count: metrics.length })}
            </span>
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-auto text-xs">
          {pending ? (
            <div className="px-5 py-4 text-tx-3">
              {t('explore.catalog.loading')}
            </div>
          ) : error ? (
            <div className="px-5 py-4 text-tx-3">
              {t('explore.catalog.load_error', {
                error:
                  error instanceof Error
                    ? error.message
                    : t('explore.catalog.unknown_error'),
              })}
            </div>
          ) : metrics.length === 0 ? (
            <div className="px-5 py-4 text-tx-3">
              {filter.trim()
                ? t('explore.catalog.no_filter_match')
                : t('explore.catalog.empty')}
            </div>
          ) : (
            <ul className="divide-y divide-bd-0">
              {metrics.map((metric) => {
                const metricType = resolveMetricType(metric);
                const selected = metric.name === selectedMetricName;
                return (
                  <li key={metric.name}>
                    <button
                      type="button"
                      onClick={() => onPickMetric(metric)}
                      aria-current={selected ? 'true' : undefined}
                      className={`grid min-h-14 w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-4 border-l-2 px-5 py-2.5 text-left transition-colors focus-visible:bg-bg-3 ${
                        selected
                          ? 'border-orange bg-bg-2'
                          : 'border-transparent hover:bg-bg-2'
                      }`}
                    >
                      <div className="min-w-0">
                        <div className="flex min-w-0 items-baseline gap-2 font-mono">
                          <MetricTypePrefix
                            type={metricType}
                            label={t(`explore.types.${metricType}`)}
                          />
                          <span className="truncate text-tx-0">{metric.name}</span>
                        </div>
                        <div className="truncate pl-[38px] text-xs text-tx-3">
                          {t('explore.catalog.metric_meta', {
                            labels: metric.labels.length,
                            fields: metric.field_count,
                          })}
                        </div>
                      </div>
                      <span className="font-sans text-xs font-semibold text-blue-soft">
                        {selected
                          ? t('explore.catalog.selected')
                          : t('explore.catalog.select')}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>

        {!pending && !error && (metrics.length > 0 || hasPrevious) ? (
          <div className="border-t border-bd-0 bg-bg-2/50">
            <CursorPagination
              pageSize={pageSize}
              pageSizeOptions={PAGE_SIZE_OPTIONS}
              hasPrevious={hasPrevious}
              hasNext={hasNext}
              ariaLabel={t('explore.catalog.pagination.aria')}
              pageSizeAriaLabel={t('explore.catalog.pagination.page_size_aria')}
              previousLabel={t('explore.catalog.pagination.previous')}
              nextLabel={t('explore.catalog.pagination.next')}
              onPrevious={onPrevious}
              onNext={onNext}
              onPageSizeChange={onPageSizeChange}
              className="px-3"
            />
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
