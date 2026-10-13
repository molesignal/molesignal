import { CircleAlert, UserRound } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { useAuthStore } from '@/stores/auth';

import type { DisplayReport, ReportFormat } from '../reportTypes';
import { runAsName } from './model';
import { useRunAs } from './useRunAs';

/**
 * States, beside the control that saves a report, whose access its deliveries read data with
 * and what saving does to that. `report` is `null` for a report that has not been saved yet;
 * `format` is the format chosen in the editor.
 *
 * A reader who cannot save is told who the report runs as, and nothing about saving.
 * Renders nothing for a format that renders no page: no user's access is involved.
 */
export function RunAsNote({
  report,
  format,
  canSave,
  className,
}: {
  report: DisplayReport | null;
  format: ReportFormat;
  canSave: boolean;
  className?: string;
}) {
  const { t } = useTranslation('reports');
  const runAs = useRunAs(report, format);
  const reader = useAuthStore((state) => state.ctx);

  // Without a saved report there is only what saving would do, and this reader cannot save.
  if (!runAs || (!report && !canSave)) return null;

  // The parts are flex items: the spaces between them are for readers of the text, the gap
  // sets them apart on screen, and a part that does not fit moves to the next line whole.
  if (runAs.kind === 'missing') {
    return (
      <p
        role="status"
        className={cn(
          'm-0 flex min-w-0 max-w-[392px] items-start gap-2 rounded-md bg-[var(--yellow-tint)] px-3 py-2 text-xs leading-relaxed text-[var(--yellow-ink)] xl:max-w-none',
          className,
        )}
      >
        <CircleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
        <span className="flex min-w-0 flex-wrap gap-x-1">
          <span>
            {t('run_as.missing', { format: runAs.format.toUpperCase() })}
          </span>{' '}
          <span>
            {t(canSave ? 'run_as.missing_fix' : 'run_as.missing_needs_saver')}
          </span>
        </span>
      </p>
    );
  }

  // The backend names the user with the report, whether or not this reader may list members.
  const profile = report?.raw.run_as_user;
  const other = runAs.kind === 'other';
  const name = other
    ? runAsName(profile, runAs.userId)
    : t('run_as.you', {
        name:
          (profile?.id === reader?.user_id && runAsName(profile, '')) ||
          reader?.display_name ||
          reader?.email ||
          reader?.user_id,
      });

  return (
    <p
      className={cn(
        'm-0 flex min-w-0 flex-wrap items-center justify-end gap-x-1.5 gap-y-0.5 text-xs leading-relaxed text-tx-2',
        className,
      )}
    >
      <UserRound className="h-3.5 w-3.5 shrink-0 text-tx-3" aria-hidden />
      <span className="text-tx-3">{t('run_as.label')}</span>{' '}
      <span
        className="min-w-0 break-words font-strong text-tx-0"
        // The email tells two people with the same name apart.
        title={other && profile?.display_name ? profile.email : undefined}
      >
        {name}
      </span>
      {other && canSave && (
        <>
          {' '}
          {/* Shown where the two parts are sure to share a line. */}
          <span className="hidden text-tx-3 lg:inline" aria-hidden>
            ·
          </span>{' '}
          <span className="text-tx-1">{t('run_as.takeover')}</span>
        </>
      )}
    </p>
  );
}
