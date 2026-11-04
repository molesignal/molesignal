import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';

/** A line about the estimate: on its way, or nothing to report. */
export function ImpactStatus({ children }: { children: React.ReactNode }) {
  return (
    <p role="status" className="font-sans text-sm text-tx-2">
      {children}
    </p>
  );
}

/** The estimate did not come. Saving is still possible, and asks for more when it is done blind. */
export function ImpactFailure({ children, onRetry }: { children: React.ReactNode; onRetry: () => void }) {
  const { t } = useTranslation('common');

  return (
    <div role="alert" className="flex flex-wrap items-center gap-x-3 gap-y-1">
      <p className="font-sans text-sm text-[var(--orange-ink)]">{children}</p>
      <ChromeButton variant="ghost" onClick={onRetry}>
        {t('actions.retry')}
      </ChromeButton>
    </div>
  );
}
