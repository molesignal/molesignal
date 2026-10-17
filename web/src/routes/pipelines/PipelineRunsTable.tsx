import type { TFunction } from 'i18next';
import type * as React from 'react';

import { DataTable } from '@/admin';
import type { PipelineRun } from '@/api/pipelines/runs';
import { ProductState } from '@/product/states';

import {
  formatRelativeMicros,
  formatRunDuration,
} from './presentation';
import { pipelineFlatTableClassName } from './Surfaces';

interface PipelineRunsTableLabels {
  state: React.ReactNode;
  started: React.ReactNode;
  duration: React.ReactNode;
  rows: React.ReactNode;
  error: React.ReactNode;
  emptyTitle: React.ReactNode;
  emptyDescription: React.ReactNode;
  /** How many events of a kind, the number already formatted. */
  count: (kind: 'scanned' | 'written' | 'dropped' | 'failed', value: string) => string;
  attempt: (attempt: number) => string;
}

export function getPipelineRunsTableLabels(
  t: TFunction<'pipelines'>,
): PipelineRunsTableLabels {
  return {
    state: t('detail.run_columns.state'),
    started: t('detail.run_columns.started'),
    duration: t('detail.run_columns.duration'),
    rows: t('detail.run_columns.rows'),
    error: t('detail.run_columns.error'),
    emptyTitle: t('detail.runs_empty_title'),
    emptyDescription: t('detail.runs_empty_description'),
    count: (kind, value) => t(`detail.run_counts.${kind}`, { value }),
    attempt: (attempt) => t('detail.run_attempt', { attempt }),
  };
}

export function PipelineRunsTable({
  rows,
  loading,
  compact = false,
  locale,
  labels,
  renderState,
}: {
  rows: PipelineRun[];
  loading: boolean;
  compact?: boolean;
  locale: string;
  labels: PipelineRunsTableLabels;
  renderState: (state: string) => React.ReactNode;
}) {
  if (loading) {
    return (
      <ProductState
        variant="loading"
        compact
        className={pipelineFlatTableClassName}
      />
    );
  }
  if (rows.length === 0) {
    return (
      <ProductState
        variant="empty"
        compact
        title={labels.emptyTitle}
        description={labels.emptyDescription}
        className={pipelineFlatTableClassName}
      />
    );
  }

  return (
    <DataTable
      className={pipelineFlatTableClassName}
      rows={rows}
      rowKey={(run) => run.id}
      columns={[
        {
          key: 'state',
          header: labels.state,
          width: 120,
          cell: (run) => (
            <div>
              {renderState(run.state)}
              {(run.attempt ?? 1) > 1 && (
                <div className="mt-0.5 font-sans text-xs text-tx-3">{labels.attempt(run.attempt ?? 1)}</div>
              )}
            </div>
          ),
        },
        {
          key: 'started',
          header: labels.started,
          width: 200,
          cell: (run) => (
            <div>
              <div className="text-tx-1">
                {formatRelativeMicros(run.started_at_micros, locale)}
              </div>
              {!compact && (
                <div className="mt-0.5 font-mono text-xs text-tx-3">
                  {new Date(run.started_at_micros / 1000).toLocaleString(locale)}
                </div>
              )}
            </div>
          ),
        },
        {
          key: 'duration',
          header: labels.duration,
          width: 130,
          cell: (run) => formatRunDuration(run.started_at_micros, run.finished_at_micros),
        },
        {
          key: 'rows',
          header: labels.rows,
          width: 200,
          cell: (run) => <RunCounts run={run} locale={locale} labels={labels} />,
        },
        {
          key: 'error',
          header: labels.error,
          // A failure says why; a run that went through may still say what went wrong on the way.
          cell: (run) => (
            <span
              className={
                run.error ? 'text-red-soft' : run.warning ? 'text-tx-2' : 'text-tx-3'
              }
            >
              {run.error ?? run.warning ?? '—'}
            </span>
          ),
        },
      ]}
    />
  );
}

/**
 * Where a run's events ended: how many it read and wrote, then the dropped
 * and failed ones when there are any.
 */
function RunCounts({
  run,
  locale,
  labels,
}: {
  run: PipelineRun;
  locale: string;
  labels: PipelineRunsTableLabels;
}) {
  const format = (value: number) => new Intl.NumberFormat(locale).format(value);
  const dropped = run.dropped_rows ?? 0;
  const failed = run.failed_rows ?? 0;

  return (
    <div className="font-sans text-xs tabular-nums">
      <div className="text-tx-1">
        {labels.count('scanned', format(run.scanned_rows))}
        {run.written_rows !== undefined && (
          <span className="text-tx-3"> · {labels.count('written', format(run.written_rows))}</span>
        )}
      </div>
      {(dropped > 0 || failed > 0) && (
        <div className="mt-0.5">
          {dropped > 0 && <span className="text-tx-3">{labels.count('dropped', format(dropped))}</span>}
          {dropped > 0 && failed > 0 && <span className="text-tx-3"> · </span>}
          {failed > 0 && <span className="text-red-soft">{labels.count('failed', format(failed))}</span>}
        </div>
      )}
    </div>
  );
}
