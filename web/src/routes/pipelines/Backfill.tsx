import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { PageHeader } from '@/admin';
import * as pipelineRunsApi from '@/api/pipelines/runs';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput, FormRow, FormSection } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

function parseDatetime(value: string): number | null {
  if (!value) return null;
  const ts = new Date(value).getTime();
  return Number.isFinite(ts) ? ts * 1000 : null;
}

export function PipelineBackfill() {
  const { t } = useTranslation('pipelines');
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const runAccess = useActionAccess({ permission: 'pipelines.run' });
  const [from, setFrom] = React.useState('');
  const [to, setTo] = React.useState('');
  const [lastJob, setLastJob] = React.useState<{ job_id: string; monitor: string } | null>(null);

  const submit = useMutation({
    mutationFn: (input: pipelineRunsApi.BackfillSubmissionInput) =>
      pipelineRunsApi.submitBackfill(id, input),
    onSuccess: (data) => {
      setLastJob(data);
      toast.success(t('flows.backfill.toast_queued', { jobIdShort: data.job_id.slice(0, 10) }));
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
        title={t('flows.backfill.title')}
        subtitle={id}
        actions={
          <ChromeButton onClick={() => navigate(`/pipelines/${encodeURIComponent(id)}/edit`)}>
            {t('flows.backfill.back_to_edit')}
          </ChromeButton>
        }
      />
      <div className="mx-auto max-w-2xl space-y-4 p-4">
        <form onSubmit={onSubmit}>
          <FormSection title={t('flows.backfill.window_title')}>
            <FormRow>
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
            </FormRow>
          </FormSection>
          <div className="flex items-center gap-3">
            <ChromeButton
              type="submit"
              variant="primary"
              disabled={runAccess.disabled || submit.isPending}
              disabledReason={!submit.isPending ? runAccess.reason : undefined}
            >
              {submit.isPending ? t('flows.backfill.submitting') : t('flows.backfill.submit_backfill')}
            </ChromeButton>
            {lastJob && (
              <a
                href={lastJob.monitor}
                className="font-sans text-xs text-blue-soft hover:underline"
              >
                {t('flows.backfill.monitor_link', { jobIdShort: lastJob.job_id.slice(0, 10) })}
              </a>
            )}
          </div>
        </form>
      </div>
    </>
  );
}
