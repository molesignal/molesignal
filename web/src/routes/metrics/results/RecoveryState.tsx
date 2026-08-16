import { useTranslation } from 'react-i18next';

import { EmptyState } from '@/shell/EmptyState';
import { ErrorState } from '@/shell/ErrorState';

import type { MetricsExploreResultsProps } from './types';

type Recovery = MetricsExploreResultsProps['recovery'];

export function MetricsQueryErrorState({
  error,
  recovery,
}: {
  error: unknown;
  recovery: Recovery;
}) {
  const { t } = useTranslation('metrics');
  return (
    <ErrorState
      error={error}
      title={t('explore.recovery.query_failed')}
      onRetry={recovery.onRetry}
      help={{
        label: t('explore.recovery.promql_docs'),
        href: recovery.docsHref,
      }}
      className="min-h-[280px] justify-center"
      data-testid="metrics-query-error"
    />
  );
}

export function MetricsNoDataState({ recovery }: { recovery: Recovery }) {
  const { t } = useTranslation('metrics');
  const widen = {
    label: t('explore.recovery.expand_time'),
    onClick: recovery.onWidenRange,
  };
  return (
    <EmptyState
      strategy="query-first"
      title={t('explore.recovery.no_data_title')}
      description={t('explore.recovery.no_data_description')}
      primaryAction={
        recovery.hasFilters
          ? {
              label: t('explore.recovery.clear_filters'),
              onClick: recovery.onClearFilters,
            }
          : widen
      }
      secondaryAction={recovery.hasFilters ? widen : undefined}
      className="min-h-[320px]"
      data-testid="metrics-no-data"
    />
  );
}
