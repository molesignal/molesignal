import { EmptyState } from '@/shell/EmptyState';
import { ErrorState } from '@/shell/ErrorState';
import { QueryState } from '@/shell/query/State';

export interface QueryRecoveryCopy {
  errorTitle: string;
  emptyTitle: string;
  emptyDescription: string;
  clearFiltersLabel: string;
  widenRangeLabel: string;
  docsLabel: string;
}

export interface QueryRecoveryActions {
  hasFilters: boolean;
  onRetry: () => void;
  onClearFilters: () => void;
  onWidenRange: () => void;
  docsHref: string;
}

/** Standard loading / empty / error contract for exploratory query surfaces. */
export function QueryRecoveryState({
  state,
  error,
  copy,
  recovery,
  className,
  testId,
}: {
  state: 'loading' | 'empty' | 'error';
  error?: unknown;
  copy: QueryRecoveryCopy;
  recovery: QueryRecoveryActions;
  className?: string | undefined;
  testId?: string | undefined;
}) {
  if (state === 'loading') {
    return (
      <QueryState
        state="loading"
        {...(className ? { className } : {})}
      />
    );
  }

  if (state === 'error') {
    return (
      <ErrorState
        error={error}
        title={copy.errorTitle}
        onRetry={recovery.onRetry}
        help={{ label: copy.docsLabel, href: recovery.docsHref }}
        className={className}
        data-testid={testId}
      />
    );
  }

  const widen = {
    label: copy.widenRangeLabel,
    onClick: recovery.onWidenRange,
  };
  return (
    <EmptyState
      strategy="query-first"
      title={copy.emptyTitle}
      description={copy.emptyDescription}
      primaryAction={
        recovery.hasFilters
          ? {
              label: copy.clearFiltersLabel,
              onClick: recovery.onClearFilters,
            }
          : widen
      }
      secondaryAction={recovery.hasFilters ? widen : undefined}
      className={className}
      data-testid={testId}
    />
  );
}
