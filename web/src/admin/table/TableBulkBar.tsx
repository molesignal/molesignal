import { X } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { IconButton } from '@/shell/chrome';

/**
 * What can be done to the rows ticked in a list at once, at the head of the
 * list: how many are ticked, the actions, a way to untick them all.
 */
export function TableBulkBar({
  count,
  onClear,
  label,
  children,
  'data-testid': testId,
}: {
  count: number;
  onClear: () => void;
  /** The name of the toolbar, for assistive technology. */
  label?: string | undefined;
  children: React.ReactNode;
  'data-testid'?: string | undefined;
}) {
  const { t } = useTranslation('common');

  return (
    <div
      data-table-bulk
      data-testid={testId}
      role="toolbar"
      aria-label={label ?? t('table.bulk_label')}
      className="flex min-h-11 flex-wrap items-center gap-2 border-b border-bd-0 bg-bg-2 px-4 py-1.5"
    >
      <span className="mr-auto font-sans text-sm font-medium text-tx-0">
        {t('table.selected', { count })}
      </span>
      {children}
      <IconButton aria-label={t('table.clear_selection')} title={t('table.clear_selection')} onClick={onClear}>
        <X className="h-3.5 w-3.5" />
      </IconButton>
    </div>
  );
}
