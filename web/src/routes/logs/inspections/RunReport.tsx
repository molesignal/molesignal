import { Check, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { LogInspection, LogInspectionRun } from '@/api/logInspections';
import { formatMicrosActive } from '@/lib/time';
import { evidenceHref } from '@/routes/agent/answer';
import { Pill } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { QueryState } from '@/shell/query/State';

import { formatRunDuration, runDurationMicros } from './model';
import { RunMark } from './RunList';
import { InspectionReport } from '../inspection/Report';

const HEADING = 'font-sans text-xs font-strong text-tx-2';

interface RunReportProps {
  inspection: LogInspection;
  run: LogInspectionRun;
  /** The moment the run was read at: a run under way is timed up to it. */
  nowMicros: number;
  copied: boolean;
  onCopy: () => void;
}

/**
 * One run of an inspection: the period it looked over, the report the model
 * wrote or the reason there is none, what it read with, and whom it told.
 */
export function RunReport({ inspection, run, nowMicros, copied, onCopy }: RunReportProps) {
  const { t, i18n } = useTranslation('logs');
  const { t: tn } = useTranslation('notify');
  const count = new Intl.NumberFormat(i18n.resolvedLanguage ?? i18n.language);
  const active = run.status === 'queued' || run.status === 'running';
  const logsHref = evidenceHref({
    kind: 'logs',
    stream: inspection.stream,
    time_range: { start_micros: run.window_start_micros, end_micros: run.window_end_micros },
  });
  const requestedBy =
    run.requested_by_user?.display_name.trim() ||
    run.requested_by_user?.email.trim() ||
    run.requested_by;

  return (
    <article
      data-log-inspection-run-report={run.id}
      data-log-inspection-run-report-status={run.status}
      className="flex min-w-0 flex-col gap-[16px] px-[16px] py-[14px] font-sans"
    >
      <header className="flex min-w-0 flex-col gap-[6px]">
        <div className="flex min-w-0 flex-wrap items-center gap-[8px]">
          <RunMark run={run} />
          <h2 className="m-0 min-w-0 flex-1 truncate text-sm font-strong text-tx-0">
            {formatMicrosActive(run.window_start_micros, false)}
            {' – '}
            {formatMicrosActive(run.window_end_micros, false)}
          </h2>
          {logsHref && !active && (
            <Link
              to={logsHref}
              className="shrink-0 rounded-sm text-xs font-strong text-[var(--blue-ink)] hover:underline focus-visible:underline focus-visible:outline-none"
            >
              {t('inspections.report.open_logs')}
            </Link>
          )}
          {run.report && (
            <CopyIconButton onClick={onCopy} label={t('explore.inspection.copy_report')} copied={copied} />
          )}
        </div>
        <p data-log-inspection-run-facts className="m-0 flex flex-wrap gap-x-[12px] gap-y-[2px] text-xs text-tx-2">
          <span>
            {run.trigger === 'manual'
              ? t('inspections.report.manual_by', { name: requestedBy ?? '—' })
              : t('inspections.report.scheduled')}
          </span>
          {run.started_at_micros ? (
            <span>
              {t('inspections.report.took', {
                duration: formatRunDuration(runDurationMicros(run, nowMicros)),
              })}
            </span>
          ) : null}
          {run.model && <span>{run.model}</span>}
          {run.prompt_tokens + run.completion_tokens > 0 && (
            <span>
              {t('inspections.report.tokens', {
                formatted: count.format(run.prompt_tokens + run.completion_tokens),
              })}
            </span>
          )}
        </p>
      </header>

      {active && (
        <QueryState
          state="loading"
          loadingLabel={t(
            run.status === 'queued' ? 'inspections.report.queued' : 'explore.inspection.waiting',
          )}
          className="min-h-32"
        />
      )}

      {run.failure && (
        <div
          role="alert"
          className="rounded-md bg-[var(--orange-tint)] px-[12px] py-[10px] text-xs leading-relaxed text-[var(--orange-ink)]"
        >
          <p className="m-0 font-strong">{t(`inspections.failures.${run.failure}`)}</p>
          {run.error && <p className="m-0 mt-[4px] break-words">{run.error}</p>}
        </div>
      )}

      {run.report && <InspectionReport content={run.report} />}

      {run.tool_calls.length > 0 && (
        <section className="flex flex-col gap-[6px]">
          <h3 className={HEADING}>{t('inspections.report.tools')}</h3>
          <ul
            data-log-inspection-run-tools
            className="m-0 flex list-none flex-col gap-[4px] rounded-md bg-[var(--control-surface)] px-[12px] py-[8px]"
          >
            {run.tool_calls.map((tool, index) => (
              <li key={index} className="flex items-center gap-[8px] text-xs text-tx-2">
                {tool.ok ? (
                  <Check className="h-3 w-3 text-[var(--green-ink)]" aria-hidden="true" />
                ) : (
                  <TriangleAlert className="h-3 w-3 text-[var(--orange-ink)]" aria-hidden="true" />
                )}
                <span className="min-w-0 truncate">
                  {t(tool.ok ? 'explore.inspection.tool_done' : 'explore.inspection.tool_error', {
                    name: tool.name,
                  })}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {run.notifications.length > 0 && (
        <section className="flex flex-col gap-[6px]">
          <h3 className={HEADING}>{t('inspections.report.notified')}</h3>
          <ul data-log-inspection-run-notifications className="m-0 flex list-none flex-col gap-[4px] p-0">
            {run.notifications.map((notification, index) => (
              <li
                key={index}
                className="flex min-w-0 flex-wrap items-center gap-[8px] rounded-md bg-[var(--control-surface)] px-[12px] py-[8px] text-xs text-tx-1"
              >
                <Pill tone={notification.delivered ? 'green' : 'red'}>
                  {t(
                    notification.delivered
                      ? 'inspections.report.delivered'
                      : 'inspections.report.undelivered',
                  )}
                </Pill>
                <span className="text-tx-2">{tn(`target_types.${notification.target_type}`)}</span>
                <span className="min-w-0 truncate">{notification.target_masked || '—'}</span>
                {notification.error && (
                  <span className="basis-full break-words text-[var(--orange-ink)]">
                    {notification.error}
                  </span>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}
    </article>
  );
}
