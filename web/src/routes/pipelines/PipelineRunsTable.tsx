import type { TFunction } from 'i18next';
import type * as React from 'react';

import { DataTable } from '@/admin';
import type { PipelineRun } from '@/api/pipelines/runs';
import { ProductState } from '@/product/states';

import { pipelineFlatTableClassName } from './CardlessSurface';
import {
  formatRelativeMicros,
  formatRunDuration,
} from './presentation';

interface PipelineRunsTableLabels {
  state: React.ReactNode;
  started: React.ReactNode;
  duration: React.ReactNode;
  rows: React.ReactNode;
  error: React.ReactNode;
  emptyTitle: React.ReactNode;
  emptyDescription: React.ReactNode;
}

export function getPipelineRunsTableLabels(
  t: TFunction<'pipelines'>,
): PipelineRunsTableLabels {
  return {
    state: t('detail.run_columns.state'),
    started: t('detail.run_columns.started'),
    duration: t('flows.history.columns.duration'),
    rows: t('detail.run_columns.rows'),
    error: t('detail.run_columns.error'),
    emptyTitle: t('detail.runs_empty_title'),
    emptyDescription: t('detail.runs_empty_description'),
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
          cell: (run) => renderState(run.state),
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
          width: 140,
          cell: (run) => new Intl.NumberFormat(locale).format(run.scanned_rows),
        },
        {
          key: 'error',
          header: labels.error,
          cell: (run) => (
            <span className={run.error ? 'text-red-soft' : 'text-tx-3'}>
              {run.error ?? '—'}
            </span>
          ),
        },
      ]}
    />
  );
}
