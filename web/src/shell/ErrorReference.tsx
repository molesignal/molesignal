import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { writeClipboardText } from '@/lib/clipboard';
import { toApiError, type ApiError } from '@/lib/http';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';

/**
 * What a person quotes when they report a failure. The backend's `X-Request-Id`
 * matches the server log exactly (it is echoed on every response); without one
 * the reference is made of the status, the code and a short time suffix.
 */
export function errorReference(apiError: ApiError): string {
  if (apiError.requestId) return apiError.requestId;
  const code = apiError.code ?? 'ERR';
  const suffix = Math.floor(Date.now() / 1000).toString(36).slice(-6).toUpperCase();
  return `${apiError.status || 'NET'}-${code}-${suffix}`;
}

/**
 * One line under a failure: the status, and the reference with its copy button.
 * A failure that never reached the server has nothing to look up, so it draws nothing.
 */
export function ErrorReference({
  error,
  className,
}: {
  error: unknown;
  className?: string | undefined;
}) {
  const { t } = useTranslation('common');
  const apiError = React.useMemo(() => toApiError(error), [error]);
  const reference = React.useMemo(() => errorReference(apiError), [apiError]);
  const [copied, setCopied] = React.useState(false);

  if (apiError.status === 0 && !apiError.requestId) return null;

  const copy = async () => {
    try {
      await writeClipboardText(reference);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // The clipboard is blocked: the reference stays on screen to be selected by hand.
    }
  };

  return (
    <div
      data-error-reference
      className={cn('flex flex-wrap items-center justify-center gap-x-1.5 font-sans text-xs text-tx-3', className)}
    >
      {apiError.status > 0 && <span className="tabular-nums">HTTP {apiError.status}</span>}
      {apiError.status > 0 && <span aria-hidden>·</span>}
      <span>{apiError.requestId ? t('error_state.request_id') : t('error_state.error_id')}</span>
      <span className="select-all tabular-nums text-tx-2">{reference}</span>
      <CopyIconButton
        onClick={() => void copy()}
        label={apiError.requestId ? t('error_state.copy_request_id') : t('error_state.copy_error_id')}
        copied={copied}
        copiedLabel={t('error_state.copied')}
        className="h-6 w-6"
        iconClassName="h-3 w-3"
        data-testid="error-copy-id"
      />
    </div>
  );
}
