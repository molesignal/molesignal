import { useTranslation } from 'react-i18next';

import type { AutomationRunStatus } from '@/api/agent/automations';
import { cn } from '@/shell/lib/cn';

import { runTone } from './model';

const INK = {
  green: 'text-[var(--green-ink)]',
  orange: 'text-[var(--orange-ink)]',
  red: 'text-[var(--red-ink)]',
  blue: 'text-[var(--blue-ink)]',
  dim: 'text-tx-3',
} as const;

/** How a run went, as a coloured word: no fill, the way statuses read in tables. */
export function RunStatusWord({ status }: { status: AutomationRunStatus }) {
  const { t } = useTranslation('agent');
  return (
    <span data-automation-run-status={status} className={cn('font-strong', INK[runTone(status)])}>
      {t(`automations.run_status.${status}`)}
    </span>
  );
}
