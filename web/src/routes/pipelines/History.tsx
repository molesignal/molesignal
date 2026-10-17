import { useQuery } from '@tanstack/react-query';
import { RotateCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import * as pipelinesApi from '@/api/pipelines';
import * as pipelineRunsApi from '@/api/pipelines/runs';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

import { getPipelineRunsTableLabels, PipelineRunsTable } from './PipelineRunsTable';
import { formatRelativeMicros } from './presentation';
import { PipelineRunState, pipelineFlatTableClassName } from './Surfaces';

/**
 * Every run of one pipeline, newest first: what it read and where the events
 * ended, and why it failed. A stretch of time waiting to be tried again is
 * said above the runs.
 */
export function PipelineHistory() {
  const { t, i18n } = useTranslation('pipelines');
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const pipeline = useQuery({
    queryKey: ['pipelines', 'get', id],
    queryFn: () => pipelinesApi.get(id),
    enabled: !!id,
    refetchInterval: 5_000,
  });
  const runs = useQuery({
    queryKey: ['pipeline-runs', id],
    queryFn: () => pipelineRunsApi.list(id),
    enabled: !!id,
    refetchInterval: 5_000,
  });
  const name = pipeline.data?.name;
  const retry = pipeline.data?.retry;

  return (
    <>
      <PageHeader
        title={name ? t('flows.history.title_named', { name }) : t('flows.history.title')}
        subtitle={t('flows.history.subtitle')}
        toolbar={
          <ChromeButton onClick={() => navigate(`/pipelines/${encodeURIComponent(id)}/edit`)}>
            {t('flows.history.back_to_edit')}
          </ChromeButton>
        }
      />
      <PageBody className="flex flex-col gap-[12px]">
        {retry && (
          <p
            data-pipeline-retry
            className={`m-0 flex items-start gap-[8px] px-[14px] py-[10px] font-sans text-sm leading-relaxed text-tx-1 ${surfacePanelClass}`}
          >
            <RotateCw aria-hidden className="mt-[3px] h-3.5 w-3.5 shrink-0 text-tx-2" />
            <span className="min-w-0">
              {t('flows.history.retry', {
                source: retry.source,
                attempt: retry.attempt,
                when: formatRelativeMicros(retry.next_at_micros, i18n.language),
              })}
              {retry.error && <span className="block text-xs text-tx-3">{retry.error}</span>}
            </span>
          </p>
        )}
        <div className={`overflow-hidden ${surfacePanelClass}`}>
          {runs.isError ? (
            <ProductState
              variant="error"
              error={runs.error}
              compact
              className={pipelineFlatTableClassName}
            />
          ) : (
            <PipelineRunsTable
              rows={runs.data ?? []}
              loading={runs.isLoading}
              locale={i18n.language}
              labels={getPipelineRunsTableLabels(t)}
              renderState={(state) => (
                <PipelineRunState
                  state={state}
                  label={t(`overview.run_states.${state}`, { defaultValue: state })}
                />
              )}
            />
          )}
        </div>
      </PageBody>
    </>
  );
}
