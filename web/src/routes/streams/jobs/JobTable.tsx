import type * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { DataTable } from '@/admin';
import type { SearchJob, SearchJobState } from '@/api/searchJobs';
import type { StreamType } from '@/api/streams';
import { formatMicrosActive } from '@/lib/time';
import { Dot, Pill, type PillTone } from '@/shell/chrome';
import { StreamTypeTag } from '@/shell/StreamTypeTag';
import { formatRelativeMicros, formatRelativeSpan } from '@/time/relative';

import {
  attemptNumber,
  formatJobDuration,
  jobDurationMicros,
  statementLine,
  submitterName,
} from './model';
import { JobRowActions } from './RowActions';

type DotTone = NonNullable<React.ComponentProps<typeof Dot>['tone']>;

// The tones a pipeline run and a stream's health already use for the same words.
const STATE_TONE: Record<SearchJobState, { pill: PillTone; dot: DotTone }> = {
  pending: { pill: 'yellow', dot: 'yellow' },
  running: { pill: 'blue', dot: 'blue' },
  done: { pill: 'green', dot: 'green' },
  failed: { pill: 'red', dot: 'red' },
  cancelled: { pill: 'dim', dot: 'dim' },
};

// A column says its name on one line: the widths are set for the cells, not for a wrapped head.
const HEADER = 'whitespace-nowrap';

export function JobStateMark({ state }: { state: SearchJobState }) {
  const { t } = useTranslation('streams');
  const tone = STATE_TONE[state];

  return (
    <Pill tone={tone.pill}>
      <Dot tone={tone.dot} />
      {t(`jobs.states.${state}`)}
    </Pill>
  );
}

export interface JobTableProps {
  jobs: SearchJob[];
  /** The moment the list was read at: running jobs are timed up to it. */
  nowMicros: number;
  /** The job something is being done to, whose controls wait. */
  busyId: string | null;
  /** Whether the reader may open a data pipeline, so a backfill can link to its own. */
  pipelinesVisible: boolean;
  /** Whether the list holds other people's jobs too, so each row names who submitted it. */
  showSubmitter: boolean;
  emptyLabel: React.ReactNode;
  onCancel: (job: SearchJob) => void;
  onRetry: (job: SearchJob) => void;
  onDownload: (job: SearchJob) => void;
  onDelete: (job: SearchJob) => void;
}

/**
 * The jobs of one page of the list. A row says what was asked (which stream,
 * which statement, over which time), who asked, and how it went; a job that
 * failed says why under its statement, where the eye already is.
 */
export function JobTable({
  jobs,
  nowMicros,
  busyId,
  pipelinesVisible,
  showSubmitter,
  emptyLabel,
  onCancel,
  onRetry,
  onDownload,
  onDelete,
}: JobTableProps) {
  const { t, i18n } = useTranslation('streams');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const count = new Intl.NumberFormat(locale);

  // The table brings its own scroll container. Under its least width, which a 1280px window
  // still holds, the columns keep their widths and the rows are scrolled sideways.
  return (
    <DataTable
      rows={jobs}
      rowKey={(job) => job.job_id}
      emptyLabel={emptyLabel}
      className="min-w-[1160px] rounded-none border-0 bg-transparent"
      columns={[
        {
          key: 'status',
          header: t('jobs.columns.status'),
          width: 100,
          headerClassName: HEADER,
          cell: (job) => <JobStateMark state={job.state} />,
        },
        {
          key: 'kind',
          header: t('jobs.columns.kind'),
          width: 72,
          headerClassName: HEADER,
          // A type is said in a word: colour is kept for the state beside it.
          cell: (job) => <Pill tone="neutral">{t(`jobs.kinds.${job.kind}`)}</Pill>,
        },
        {
          key: 'job',
          header: t('jobs.columns.job'),
          // The one column that gives: it takes the room the others leave and cuts its text to it.
          className: 'w-full max-w-0',
          headerClassName: HEADER,
          cell: (job) => <JobSummary job={job} pipelinesVisible={pipelinesVisible} />,
        },
        {
          key: 'time_range',
          header: t('jobs.columns.time_range'),
          width: 132,
          headerClassName: HEADER,
          cell: (job) =>
            job.request ? (
              <div className="tabular-nums text-tx-1">
                <div>{formatMicrosActive(job.request.time_range.start, false)}</div>
                <div className="mt-0.5 text-tx-2">
                  {formatMicrosActive(job.request.time_range.end, false)}
                </div>
              </div>
            ) : (
              '—'
            ),
        },
        {
          key: 'submitted',
          header: t('jobs.columns.submitted'),
          width: 128,
          headerClassName: HEADER,
          cell: (job) => (
            <div className="min-w-0">
              {/* In a list of one's own jobs the name would be the same on every row. */}
              {showSubmitter && (
                <div
                  className="mb-0.5 truncate text-tx-1"
                  title={job.submitted_by?.email || job.user_id}
                >
                  {submitterName(job)}
                </div>
              )}
              <div
                className={showSubmitter ? 'truncate text-tx-2' : 'truncate text-tx-1'}
                title={formatMicrosActive(job.submitted_at_micros)}
              >
                {formatRelativeMicros(job.submitted_at_micros, locale, nowMicros)}
              </div>
            </div>
          ),
        },
        {
          key: 'duration',
          header: t('jobs.columns.duration'),
          width: 120,
          headerClassName: HEADER,
          cell: (job) => (
            <div>
              <div className="tabular-nums text-tx-1">
                {formatJobDuration(jobDurationMicros(job, nowMicros))}
              </div>
              <div
                className="mt-0.5 text-tx-2"
                title={
                  job.finished_at_micros
                    ? formatMicrosActive(job.finished_at_micros)
                    : undefined
                }
              >
                {job.finished_at_micros
                  ? t('jobs.cells.finished_at', {
                      time: formatRelativeMicros(job.finished_at_micros, locale, nowMicros),
                    })
                  : t('jobs.cells.not_finished')}
              </div>
            </div>
          ),
        },
        {
          key: 'rows',
          header: t('jobs.columns.rows'),
          width: 84,
          className: 'text-right',
          headerClassName: `${HEADER} text-right`,
          cell: (job) => (
            <span className="tabular-nums text-tx-1">
              {job.result_rows == null ? '—' : count.format(job.result_rows)}
            </span>
          ),
        },
        {
          key: 'attempt',
          header: t('jobs.columns.attempt'),
          width: 56,
          className: 'text-right',
          headerClassName: `${HEADER} text-right`,
          cell: (job) => (
            <span
              className="tabular-nums text-tx-1"
              title={t('jobs.cells.attempt_title', { number: attemptNumber(job) })}
            >
              {attemptNumber(job)}
            </span>
          ),
        },
        {
          key: 'expires',
          header: t('jobs.columns.expires'),
          width: 84,
          headerClassName: HEADER,
          cell: (job) =>
            job.expires_at_micros <= nowMicros ? (
              <span className="text-tx-2" title={t('jobs.cells.expired_title')}>
                {t('jobs.cells.expired')}
              </span>
            ) : (
              <span className="text-tx-2" title={formatMicrosActive(job.expires_at_micros)}>
                {formatRelativeSpan(job.expires_at_micros, nowMicros, locale)}
              </span>
            ),
        },
        {
          key: 'actions',
          header: <span className="sr-only">{t('jobs.columns.actions')}</span>,
          width: 92,
          className: 'overflow-visible text-right',
          cell: (job) => (
            <JobRowActions
              job={job}
              busy={busyId === job.job_id}
              onCancel={() => onCancel(job)}
              onRetry={() => onRetry(job)}
              onDownload={() => onDownload(job)}
              onDelete={() => onDelete(job)}
            />
          ),
        },
      ]}
    />
  );
}

function JobSummary({ job, pipelinesVisible }: { job: SearchJob; pipelinesVisible: boolean }) {
  const { t } = useTranslation('streams');
  const statement = statementLine(job.request?.statement);
  const stream = job.request?.stream;

  return (
    <div data-search-job={job.job_id} data-search-job-state={job.state} className="min-w-0 py-1">
      <div className="flex min-w-0 items-center gap-[8px]">
        {/* The stream's name is what the row is read by: short of room, the id gives way and
            the name is cut only once it would take most of the line. */}
        {stream && (
          <span className="max-w-[60%] shrink-0 truncate font-strong text-tx-0" title={stream.name}>
            {stream.name}
          </span>
        )}
        {stream && <StreamTypeTag type={stream.stream_type as StreamType} className="shrink-0" />}
        <span className="min-w-0 truncate font-mono text-xs text-tx-3" title={job.job_id}>
          {job.job_id.slice(0, 10)}
        </span>
        {job.pipeline_id && pipelinesVisible && (
          <Link
            to={`/pipelines/${encodeURIComponent(job.pipeline_id)}`}
            className="shrink-0 rounded-sm text-xs font-strong text-indigo-soft hover:text-indigo focus-visible:bg-indigo-dim focus-visible:outline-none"
          >
            {t('jobs.cells.pipeline')}
          </Link>
        )}
      </div>
      <div className="mt-0.5 truncate text-xs text-tx-2" title={statement || undefined}>
        {statement || t('jobs.cells.request_unreadable')}
      </div>
      {job.error && (
        <div className="mt-0.5 truncate text-xs text-red-soft" title={job.error}>
          {t('jobs.cells.error', { error: job.error })}
        </div>
      )}
    </div>
  );
}
