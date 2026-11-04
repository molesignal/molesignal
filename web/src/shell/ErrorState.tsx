import { BookOpen, MessageSquareWarning } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { toApiError, type ApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';

/**
 * The failure of one thing a person just did (a query, a preview): the same
 * state as any other failure, with its status and reference, plus the ways out
 * that fit this action: try again, read the docs, report it.
 */

interface ErrorStateProps {
  error: unknown;
  /** What I tried to do — keep it short, no ellipsis. */
  title: string;
  onRetry?: (() => void) | undefined;
  onReport?: ((apiError: ApiError) => void) | undefined;
  help?: { label: string; href: string } | undefined;
  className?: string | undefined;
  'data-testid'?: string | undefined;
}

export function ErrorState({
  error,
  title,
  onRetry,
  onReport,
  help,
  className,
  'data-testid': testId,
}: ErrorStateProps) {
  const { t } = useTranslation('common');
  const apiError = toApiError(error);

  return (
    <ProductState
      variant="error"
      title={title}
      description={apiError.message || t('error_state.unknown')}
      error={error}
      onRetry={onRetry}
      className={className}
      data-testid={testId}
      action={
        (onReport || help) && (
          <>
            {onReport && (
              <ChromeButton onClick={() => onReport(apiError)} data-testid="error-report">
                <MessageSquareWarning className="h-3.5 w-3.5" />
                {t('error_state.report')}
              </ChromeButton>
            )}
            {help && (
              <a
                href={help.href}
                target="_blank"
                rel="noopener noreferrer"
                data-testid="error-help"
                className="ms-press inline-flex h-9 items-center gap-2 rounded-md bg-bg-2 px-4 font-sans text-sm font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none"
              >
                <BookOpen className="h-3.5 w-3.5" aria-hidden />
                {help.label}
              </a>
            )}
          </>
        )
      }
    />
  );
}
