import { Search, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { TimeRangeControl } from '@/time/TimePicker';

import type { ApmUrlFilters } from './model';

export function ApmFilters({
  filters,
  setFilter,
  clearFilters,
  showSearch = true,
  showCategory = false,
  showService = true,
}: {
  filters: ApmUrlFilters;
  setFilter: (key: keyof ApmUrlFilters, value: string) => void;
  clearFilters: () => void;
  showSearch?: boolean;
  showCategory?: boolean;
  showService?: boolean;
}) {
  const { t } = useTranslation('apm');
  const control =
    'h-8 rounded-md border border-bd-0 bg-bg-1 px-2.5 text-xs text-tx-1 outline-none transition-colors placeholder:text-tx-3 hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:text-tx-0';
  const hasFilters = Boolean(
    filters.namespace ||
      filters.service ||
      filters.environment ||
      filters.version ||
      filters.search ||
      filters.category,
  );
  return (
    <div className="flex flex-wrap items-center gap-2">
      {showSearch && (
        <label className="relative">
          <span className="sr-only">{t('filters.search')}</span>
          <Search
            aria-hidden
            className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3"
          />
          <input
            value={filters.search}
            onChange={(event) => setFilter('search', event.target.value)}
            placeholder={t('filters.search')}
            className={cn(control, 'w-48 pl-8')}
          />
        </label>
      )}
      <input
        aria-label={t('filters.namespace')}
        value={filters.namespace}
        onChange={(event) => setFilter('namespace', event.target.value)}
        placeholder={t('filters.namespace')}
        className={cn(control, 'w-32')}
      />
      {showService && (
        <input
          aria-label={t('filters.service')}
          value={filters.service}
          onChange={(event) => setFilter('service', event.target.value)}
          placeholder={t('filters.service')}
          className={cn(control, 'w-36')}
        />
      )}
      <input
        aria-label={t('filters.environment')}
        value={filters.environment}
        onChange={(event) => setFilter('environment', event.target.value)}
        placeholder={t('filters.environment')}
        className={cn(control, 'w-32')}
      />
      <input
        aria-label={t('filters.version')}
        value={filters.version}
        onChange={(event) => setFilter('version', event.target.value)}
        placeholder={t('filters.version')}
        className={cn(control, 'w-28')}
      />
      {showCategory && (
        <select
          aria-label={t('filters.category')}
          value={filters.category}
          onChange={(event) => setFilter('category', event.target.value)}
          className={control}
        >
          <option value="">{t('filters.all_categories')}</option>
          {[
            'service',
            'database',
            'cache',
            'messaging',
            'external_http',
            'external_rpc',
            'other',
          ].map((category) => (
            <option key={category} value={category}>
              {t(`dependency_categories.${category}`)}
            </option>
          ))}
        </select>
      )}
      <select
        aria-label={t('filters.resolution')}
        value={filters.resolution}
        onChange={(event) => setFilter('resolution', event.target.value)}
        className={control}
      >
        <option value="auto">{t('filters.resolution_auto')}</option>
        <option value="minute">{t('filters.resolution_minute')}</option>
        <option value="hour">{t('filters.resolution_hour')}</option>
      </select>
      <TimeRangeControl
        align="end"
        className="h-8 max-w-[180px] border-bd-0 bg-bg-1 px-2.5 text-xs"
      />
      {hasFilters && (
        <button
          type="button"
          onClick={clearFilters}
          className="inline-flex h-8 items-center gap-1 rounded-md px-2 text-xs font-strong text-tx-2 outline-none hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0"
        >
          <X aria-hidden className="h-3.5 w-3.5" />
          {t('filters.clear')}
        </button>
      )}
    </div>
  );
}
