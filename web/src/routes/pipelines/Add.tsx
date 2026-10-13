import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as pipelinesApi from '@/api/pipelines';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { toast } from '@/shell/ui/sonner';

import { PipelineForm, saveFailureMessage } from './PipelineForm';

export function PipelineAdd() {
  const { t } = useTranslation('pipelines');
  const navigate = useNavigate();
  const qc = useQueryClient();
  const createAccess = useActionAccess({ permission: 'pipelines.create' });
  const create = useMutation({
    mutationFn: (payload: pipelinesApi.PipelineInput) => pipelinesApi.create(payload),
    onSuccess: (resp) => {
      toast.success(t('flows.add.toast_created'));
      void qc.invalidateQueries({ queryKey: ['pipelines', 'list'] });
      navigate(`/pipelines/${encodeURIComponent(resp.id)}/edit`);
    },
    onError: (err) => toast.error(saveFailureMessage(err, t)),
  });

  return (
    <>
      <PageHeader
        title={t('flows.add.title')}
        subtitle={t('workspace.subtitle')}
        toolbar={
          <>
            <ChromeButton onClick={() => navigate('/pipelines')}>{t('flows.add.cancel')}</ChromeButton>
            <ChromeButton
              type="submit"
              form="pipeline-form"
              variant="primary"
              disabled={createAccess.disabled || create.isPending}
              disabledReason={!create.isPending ? createAccess.reason : undefined}
            >
              {create.isPending ? t('flows.add.saving') : t('flows.add.submit')}
            </ChromeButton>
          </>
        }
      />
      <PageBody padded={false}>
        <PipelineForm
          formId="pipeline-form"
          disabled={createAccess.disabled}
          disabledReason={createAccess.reason}
          onSubmit={(p) => {
            if (createAccess.allowed) create.mutate(p);
          }}
        />
      </PageBody>
    </>
  );
}
