import { CircleStop, Download, RotateCw, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { SearchJob } from '@/api/searchJobs';
import { IconButton } from '@/shell/chrome';

import { jobAction } from './model';

/**
 * What can be done to a job from its row: the one thing its state calls for
 * (stop it, run it again, or take its results), and deleting it.
 */
export function JobRowActions({
  job,
  busy,
  onCancel,
  onRetry,
  onDownload,
  onDelete,
}: {
  job: SearchJob;
  /** Something is being done to this job: its controls wait for it. */
  busy: boolean;
  onCancel: () => void;
  onRetry: () => void;
  onDownload: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation('streams');
  const { t: tc } = useTranslation('common');
  const id = job.job_id.slice(0, 10);
  const action = jobAction(job.state);
  const pendingReason = busy ? tc('access.operation_pending') : undefined;
  const empty = action === 'download' && job.result_rows === 0;

  return (
    <div className="flex items-center justify-end gap-[2px]">
      {action === 'cancel' && (
        <IconButton
          aria-label={t('jobs.actions.cancel_named', { id })}
          title={t('jobs.actions.cancel')}
          disabled={busy}
          disabledReason={pendingReason}
          onClick={onCancel}
        >
          <CircleStop className="h-3.5 w-3.5" />
        </IconButton>
      )}
      {action === 'retry' && (
        <IconButton
          aria-label={t('jobs.actions.retry_named', { id })}
          title={t('jobs.actions.retry')}
          disabled={busy}
          disabledReason={pendingReason}
          onClick={onRetry}
        >
          <RotateCw className="h-3.5 w-3.5" />
        </IconButton>
      )}
      {action === 'download' && (
        <IconButton
          aria-label={t('jobs.actions.download_named', { id })}
          title={t('jobs.actions.download')}
          disabled={busy || empty}
          disabledReason={pendingReason ?? (empty ? t('jobs.actions.download_empty') : undefined)}
          onClick={onDownload}
        >
          <Download className="h-3.5 w-3.5" />
        </IconButton>
      )}
      <IconButton
        aria-label={t('jobs.actions.delete_named', { id })}
        title={t('jobs.actions.delete')}
        disabled={busy}
        disabledReason={pendingReason}
        onClick={onDelete}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </IconButton>
    </div>
  );
}
