import { useMutation, useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import * as pipelinesApi from '@/api/pipelines';
import * as pipelineRunsApi from '@/api/pipelines/runs';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { toast } from '@/shell/ui/sonner';

function parseDatetime(value: string): number | null {
  if (!value) return null;
  const ts = new Date(value).getTime();
  return Number.isFinite(ts) ? ts * 1000 : null;
}

/**
 * Runs a pipeline again over a stretch of the past: every source it reads gets
 * a job of its own, which goes through the steps and writes where a run writes.
 * The pipeline's own read positions are not moved.
 */
export function PipelineBackfill() {
  const { t } = useTranslation('pipelines');
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const runAccess = useActionAccess({ permission: 'pipelines.run' });
  const [from, setFrom] = React.useState('');
  const [to, setTo] = React.useState('');
  const [jobs, setJobs] = React.useState<pipelineRunsApi.BackfillJob[]>([]);
  const pipeline = useQuery({
    queryKey: ['pipelines', 'get', id],
    queryFn: () => pipelinesApi.get(id),
    enabled: !!id,
  });
  const name = pipeline.data?.name;

  const submit = useMutation({
    mutationFn: (input: pipelineRunsApi.BackfillSubmissionInput) =>
      pipelineRunsApi.submitBackfill(id, input),
    onSuccess: (data) => {
      setJobs(data.jobs);
      toast.success(t('flows.backfill.toast_queued', { count: data.jobs.length }));
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const onSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!runAccess.allowed) return;
    const start = parseDatetime(from);
    const end = parseDatetime(to);
    if (start === null || end === null) {
      toast.error(t('flows.backfill.toast_invalid'));
      return;
    }
    if (end <= start) {
      toast.error(t('flows.backfill.toast_to_before_from'));
      return;
    }
    submit.mutate({ start_micros: start, end_micros: end });
  };

  return (
    <>
      <PageHeader
        title={name ? t('flows.backfill.title_named', { name }) : t('flows.backfill.title')}
        subtitle={t('flows.backfill.subtitle')}
        toolbar={
          <>
            <ChromeButton onClick={() => navigate(`/pipelines/${encodeURIComponent(id)}/edit`)}>
              {t('flows.backfill.back_to_edit')}
            </ChromeButton>
            <ChromeButton
              type="submit"
              form="backfill-form"
              variant="primary"
              disabled={runAccess.disabled || submit.isPending}
              disabledReason={!submit.isPending ? runAccess.reason : undefined}
            >
              {submit.isPending ? t('flows.backfill.submitting') : t('flows.backfill.submit_backfill')}
            </ChromeButton>
          </>
        }
      />
      <PageBody>
        <form
          id="backfill-form"
          onSubmit={onSubmit}
          aria-label={t('flows.backfill.window_title')}
          className={`px-[16px] pb-[16px] pt-[12px] ${surfacePanelClass}`}
        >
          <h2 className="m-0 font-sans text-md font-strong text-tx-0">{t('flows.backfill.window_title')}</h2>
          <p className="m-0 mt-[2px] font-sans text-xs leading-relaxed text-tx-3">{t('flows.backfill.window_hint')}</p>
          <div className="mt-[12px] grid max-w-[760px] gap-x-[16px] gap-y-[12px] sm:grid-cols-2">
            <FormField label={t('flows.backfill.from_label')} hint={t('flows.backfill.from_hint')}>
              <FormInput
                value={from}
                onChange={(e) => setFrom(e.target.value)}
                placeholder={t('flows.backfill.from_placeholder')}
                disabled={runAccess.disabled}
                disabledReason={runAccess.reason}
              />
            </FormField>
            <FormField label={t('flows.backfill.to_label')} hint={t('flows.backfill.to_hint')}>
              <FormInput
                value={to}
                onChange={(e) => setTo(e.target.value)}
                placeholder={t('flows.backfill.to_placeholder')}
                disabled={runAccess.disabled}
                disabledReason={runAccess.reason}
              />
            </FormField>
          </div>
          {jobs.length > 0 && (
            <ul data-backfill-jobs className="m-0 mt-[14px] list-none space-y-[4px] p-0">
              {jobs.map((job) => (
                <li key={job.job_id} className="font-sans text-xs text-tx-2">
                  <a href={job.monitor} className="text-[var(--blue-ink)] hover:underline">
                    {t('flows.backfill.monitor_link', {
                      source: job.source,
                      jobIdShort: job.job_id.slice(0, 10),
                    })}
                  </a>
                </li>
              ))}
            </ul>
          )}
        </form>
      </PageBody>
    </>
  );
}
