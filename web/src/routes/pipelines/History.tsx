import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { DataTable, EmptyState, PageHeader } from '@/admin';
import * as pipelineRunsApi from '@/api/pipelines/runs';
import { formatMicrosActive } from '@/lib/time';
import { ChromeButton } from '@/shell/chrome';
import { QueryState, queryStateFor } from '@/shell/query/State';

function durationLabel(start: number, end: number | null): string {
  if (!end) return '—';
  const ms = (end - start) / 1000;
  if (ms < 1000) return `${ms.toFixed(0)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${(ms / 60_000).toFixed(1)} min`;
}

export function PipelineHistory() {
  const { t } = useTranslation('pipelines');
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const q = useQuery({
    queryKey: ['pipeline-runs', id],
    queryFn: () => pipelineRunsApi.list(id),
    enabled: !!id,
    refetchInterval: 5_000,
  });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });

  return (
    <>
      <PageHeader
        title={t('flows.history.title')}
        subtitle={id}
        actions={
          <ChromeButton onClick={() => navigate(`/pipelines/${encodeURIComponent(id)}/edit`)}>
            {t('flows.history.back_to_edit')}
          </ChromeButton>
        }
      />
      <div className="p-4">
        {state ? (
          <QueryState
            state={state}
            error={q.error}
            empty={<EmptyState title={t('flows.history.empty_title')} description={t('flows.history.empty_description')} />}
          />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => r.id}
            columns={[
              {
                key: 'id',
                header: t('flows.history.columns.run_id'),
                cell: (r) => <span className="font-sans text-xs text-tx-1">{r.id.slice(0, 10)}</span>,
                width: 120,
              },
              { key: 'state', header: t('flows.history.columns.state'), cell: (r) => r.state, width: 110 },
              {
                key: 'started',
                header: t('flows.history.columns.started'),
                cell: (r) => formatMicrosActive(r.started_at_micros),
                width: 200,
              },
              {
                key: 'duration',
                header: t('flows.history.columns.duration'),
                cell: (r) => durationLabel(r.started_at_micros, r.finished_at_micros),
                width: 120,
              },
              { key: 'scanned', header: t('flows.history.columns.scanned'), cell: (r) => r.scanned_rows, width: 130 },
              { key: 'error', header: t('flows.history.columns.error'), cell: (r) => r.error ?? '—' },
            ]}
          />
        )}
      </div>
    </>
  );
}
