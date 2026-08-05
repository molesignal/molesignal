import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ListChecks } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { PageHeader } from '@/admin';
import * as pipelinesApi from '@/api/pipelines';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

import { PipelineForm } from './PipelineForm';

export function PipelineAdd() {
  const { t } = useTranslation('pipelines');
  const navigate = useNavigate();
  const qc = useQueryClient();
  const createAccess = useActionAccess({ permission: 'pipelines.create' });
  const [validationRequest, setValidationRequest] = React.useState(0);
  const create = useMutation({
    mutationFn: (payload: pipelinesApi.PipelineInput) => pipelinesApi.create(payload),
    onSuccess: (resp) => {
      toast.success(t('flows.add.toast_created'));
      void qc.invalidateQueries({ queryKey: ['pipelines', 'list'] });
      navigate(`/pipelines/${encodeURIComponent(resp.id)}/edit`);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <PageHeader
        title={t('flows.add.title')}
        subtitle={t('workspace.subtitle')}
        actions={
          <>
            <ChromeButton onClick={() => navigate('/pipelines')}>{t('flows.add.cancel')}</ChromeButton>
            <ChromeButton
              disabled={createAccess.disabled}
              disabledReason={createAccess.reason}
              onClick={() => {
                if (createAccess.allowed) {
                  setValidationRequest((request) => request + 1);
                }
              }}
            >
              <ListChecks className="h-3.5 w-3.5" />
              {t('workspace.validate')}
            </ChromeButton>
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
      <div className="bg-bg-0 p-3">
        <PipelineForm
          formId="pipeline-form"
          validationRequest={validationRequest}
          disabled={createAccess.disabled}
          disabledReason={createAccess.reason}
          onSubmit={(p) => {
            if (createAccess.allowed) create.mutate(p);
          }}
        />
      </div>
    </>
  );
}
