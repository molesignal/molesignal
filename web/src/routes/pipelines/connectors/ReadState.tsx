import { TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { Connector } from '@/api/connectors';
import { formatMicrosActive } from '@/lib/time';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

/** Beside when a source last received: that its last pull failed, which the tooltip says. */
export function ReadStateMark({ connector }: { connector: Connector }) {
  const { t } = useTranslation('pipelines');
  const failure = connector.readState?.lastError;

  if (!failure) return null;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          data-connector-failing
          role="img"
          aria-label={t('connectors.read.failing')}
          className="inline-flex shrink-0 text-[var(--red-ink)]"
        >
          <TriangleAlert aria-hidden className="h-3.5 w-3.5" />
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-[420px] break-words">{failure.text}</TooltipContent>
    </Tooltip>
  );
}

/** How a saved source's reading goes: its latest failure, and what it last skipped. */
export function ReadStateNote({ connector }: { connector: Connector }) {
  const { t } = useTranslation('pipelines');
  const { lastError: failure, lastWarning: warning } = connector.readState ?? {};

  if (!failure && !warning) return null;

  return (
    <div data-connector-read-state className="grid gap-[6px] font-sans text-xs leading-relaxed">
      {failure && (
        <p className="m-0 break-words text-[var(--red-ink)]">
          {t('connectors.read.failed', { when: formatMicrosActive(failure.atTime), reason: failure.text })}
        </p>
      )}
      {warning && (
        <p className="m-0 break-words text-tx-2">
          {t('connectors.read.skipped', { when: formatMicrosActive(warning.atTime), reason: warning.text })}
        </p>
      )}
    </div>
  );
}
