import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import { toApiError } from '@/lib/http';
import { toast } from '@/shell/ui/sonner';

import { runOverRows } from './model';

/**
 * One action over the rows ticked in a list, for a backend with no bulk call.
 * Says how it went; the selection is cleared only if every row took it, so what
 * failed stays ticked and can be tried again.
 */
export function useBulkRun<T>({
  rows,
  onSettled,
  onAllDone,
}: {
  rows: readonly T[];
  /** Refresh the list. */
  onSettled: () => void | Promise<unknown>;
  onAllDone: () => void;
}) {
  const { t } = useTranslation('common');

  return useMutation({
    mutationFn: (act: (row: T) => Promise<unknown>) => runOverRows(rows, act),
    onSuccess: async (outcome) => {
      if (outcome.failed === 0) {
        toast.success(t('table.bulk_done', { count: outcome.done }));
        onAllDone();
      } else {
        toast.error(
          t('table.bulk_failed', {
            count: outcome.failed,
            total: outcome.done + outcome.failed,
            reason: toApiError(outcome.firstError).message,
          }),
        );
      }
      await onSettled();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
}
