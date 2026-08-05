import { AlertCircle, ChevronDown, ChevronRight, MessageSquareWarning, RotateCw } from 'lucide-react';
import * as React from 'react';

import { toApiError, type ApiError } from '@/lib/http';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';

/**
 * ErrorState — brief Principle "Empty/Error/Loading 都是头等公民".
 *
 * Three-band structure (per brief):
 *   1. What I did            — title summarizing the failed action
 *   2. What you can do       — Retry / Copy error ID / Report buttons
 *   3. Where the details are — collapsible block exposing status, code,
 *                              message, optional stack
 *
 * NOT a toast. Toasts are transient feedback. This is an in-place obstacle
 * that tells the SRE which lever to pull. Use Toast ONLY for transient
 * confirmations ("query saved"), never as the primary error channel.
 */

interface ErrorStateProps {
  error: unknown;
  /** What I tried to do — keep it short, no ellipsis. */
  title: string;
  onRetry?: (() => void) | undefined;
  onReport?: ((apiError: ApiError) => void) | undefined;
  className?: string | undefined;
  'data-testid'?: string | undefined;
}

export function ErrorState({
  error,
  title,
  onRetry,
  onReport,
  className,
  'data-testid': testId,
}: ErrorStateProps) {
  const apiError = React.useMemo(() => toApiError(error), [error]);
  const errorId = React.useMemo(() => generateErrorId(apiError), [apiError]);
  const [detailsOpen, setDetailsOpen] = React.useState(false);
  const [copied, setCopied] = React.useState(false);

  const handleCopy = React.useCallback(async () => {
    try {
      await navigator.clipboard.writeText(errorId);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // clipboard is blocked — degrade silently rather than nesting a
      // second error state inside an error state.
    }
  }, [errorId]);

  return (
    <div
      role="alert"
      data-testid={testId}
      className={cn(
        'flex w-full flex-col gap-3 rounded-md border border-bd-1 bg-bg-1 p-4',
        className,
      )}
    >
      {/* Band 1: what I did */}
      <div className="flex items-start gap-3">
        <AlertCircle aria-hidden className="mt-0.5 h-5 w-5 shrink-0 stroke-[1.6] text-red" />
        <div className="min-w-0 flex-1">
          <h3 className="font-sans text-sm font-display-strong text-tx-0">{title}</h3>
          <p className="mt-0.5 truncate font-sans text-xs text-tx-2">
            {apiError.status > 0 ? `HTTP ${apiError.status} — ` : ''}
            {apiError.message || 'Unknown error'}
          </p>
        </div>
      </div>

      {/* Band 2: what you can do */}
      <div className="flex flex-wrap items-center gap-2">
        {onRetry && (
          <ActionButton
            onClick={onRetry}
            variant="primary"
            icon={<RotateCw className="h-3 w-3" />}
            label="Retry"
            testid="error-retry"
          />
        )}
        <CopyIconButton
          onClick={() => void handleCopy()}
          label="Copy error ID"
          copied={copied}
          copiedLabel="Copied"
          data-testid="error-copy-id"
        />
        {onReport && (
          <ActionButton
            onClick={() => onReport(apiError)}
            variant="secondary"
            icon={<MessageSquareWarning className="h-3 w-3" />}
            label="Report"
            testid="error-report"
          />
        )}
      </div>

      {/* Band 3: where the details are */}
      <details
        open={detailsOpen}
        onToggle={(e) => setDetailsOpen((e.target as HTMLDetailsElement).open)}
        className="rounded border border-bd-0 bg-bg-2"
      >
        <summary
          className={cn(
            'flex cursor-pointer list-none items-center gap-1.5 px-2.5 py-1.5 font-sans text-xs font-strong text-tx-2',
            'hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo focus-visible:ring-inset',
            '[&::-webkit-details-marker]:hidden',
          )}
        >
          {detailsOpen ? (
            <ChevronDown aria-hidden className="h-3 w-3" />
          ) : (
            <ChevronRight aria-hidden className="h-3 w-3" />
          )}
          <span>Details</span>
          <span className="ml-auto select-all font-sans tabular-nums text-tx-3">{errorId}</span>
        </summary>
        <dl className="grid grid-cols-[80px_1fr] gap-x-3 gap-y-1 border-t border-bd-0 px-2.5 py-2 font-sans text-xs leading-relaxed">
          <Detail label="Status" value={apiError.status > 0 ? String(apiError.status) : '—'} />
          <Detail label="Code" value={apiError.code ?? '—'} mono />
          <Detail label="Message" value={apiError.message || '(empty)'} wrap />
          <Detail label="Error ID" value={errorId} mono selectable />
        </dl>
      </details>
    </div>
  );
}

function ActionButton({
  onClick,
  icon,
  label,
  variant,
  testid,
}: {
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
  variant: 'primary' | 'secondary';
  testid?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-testid={testid}
      className={cn(
        'inline-flex h-8 items-center gap-1.5 rounded-md px-2.5 font-sans text-xs font-strong',
        'transition-colors duration-fast ease-default',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo focus-visible:ring-offset-1 focus-visible:ring-offset-bg-1',
        variant === 'primary'
          ? 'bg-indigo text-white hover:bg-indigo-soft'
          : 'border border-bd-1 bg-bg-2 text-tx-1 hover:bg-bg-3 hover:text-tx-0',
      )}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
}

function Detail({
  label,
  value,
  mono,
  selectable,
  wrap,
}: {
  label: string;
  value: string;
  mono?: boolean;
  selectable?: boolean;
  wrap?: boolean;
}) {
  return (
    <>
      <dt className="text-tx-3">{label}</dt>
      <dd
        className={cn(
          'min-w-0 text-tx-1',
          mono && 'font-sans tabular-nums',
          selectable && 'select-all',
          wrap ? 'break-words' : 'truncate',
        )}
      >
        {value}
      </dd>
    </>
  );
}

/**
 * Build the error ID surfaced to the user (and copied via "Copy error ID").
 * Prefer the backend's `X-Request-Id` (echoed on every response via
 * PropagateRequestIdLayer) so the value matches the server log exactly. Only
 * when it's absent — e.g. a network error that never reached the server — do we
 * synthesize from status + code + a short timestamp suffix.
 */
function generateErrorId(apiError: ApiError): string {
  if (apiError.requestId) return apiError.requestId;
  const status = apiError.status || 'NET';
  const code = apiError.code ?? 'ERR';
  const suffix = Math.floor(Date.now() / 1000).toString(36).slice(-6).toUpperCase();
  return `${status}-${code}-${suffix}`;
}
