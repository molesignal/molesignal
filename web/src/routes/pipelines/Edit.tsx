import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ListChecks, MoreHorizontal } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog, PageHeader } from '@/admin';
import * as pipelinesApi from '@/api/pipelines';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { QueryState, queryStateFor } from '@/shell/query/State';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';

import { PipelineForm } from './PipelineForm';

export function PipelineEdit() {
  const { t } = useTranslation('pipelines');
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const editAccess = useActionAccess({ permission: 'pipelines.edit' });
  const runAccess = useActionAccess({ permission: 'pipelines.run' });
  const pauseAccess = useActionAccess({ permission: 'pipelines.pause' });
  const deleteAccess = useActionAccess({ permission: 'pipelines.delete' });
  const [confirmDelete, setConfirmDelete] = React.useState(false);
  const [validationRequest, setValidationRequest] = React.useState(0);

  const q = useQuery({
    queryKey: ['pipelines', 'get', id],
    queryFn: () => pipelinesApi.get(id),
    enabled: !!id,
  });
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: q.data });

  const update = useMutation({
    mutationFn: (payload: pipelinesApi.PipelineInput) => pipelinesApi.update(id, payload),
    onSuccess: () => {
      toast.success(t('flows.edit.toast_updated'));
      void qc.invalidateQueries({ queryKey: ['pipelines'] });
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const remove = useMutation({
    mutationFn: () => pipelinesApi.remove(id),
    onSuccess: () => {
      toast.success(t('flows.edit.toast_deleted'));
      void qc.invalidateQueries({ queryKey: ['pipelines'] });
      navigate('/pipelines');
    },
  });

  return (
    <>
      <PageHeader
        title={q.data ? t('drawer.edit_title', { name: q.data.name }) : t('drawer.edit_title', { name: '' })}
        subtitle={t('workspace.subtitle')}
        actions={
          <>
            <ChromeButton onClick={() => navigate('/pipelines')}>{t('flows.edit.back')}</ChromeButton>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <ChromeButton aria-label={t('workspace.more_actions')}>
                  <MoreHorizontal className="h-4 w-4" />
                  {t('workspace.more_actions')}
                </ChromeButton>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-40">
                <DropdownMenuItem onSelect={() => navigate(`/pipelines/${encodeURIComponent(id)}/history`)}>
                  {t('flows.edit.history')}
                </DropdownMenuItem>
                <DropdownMenuItem
                  disabled={runAccess.disabled}
                  disabledReason={runAccess.reason}
                  onSelect={() => {
                    if (runAccess.allowed) {
                      navigate(`/pipelines/${encodeURIComponent(id)}/backfill`);
                    }
                  }}
                >
                  {t('flows.edit.backfill')}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  className="text-red-soft focus:text-red-soft"
                  disabled={deleteAccess.disabled}
                  disabledReason={deleteAccess.reason}
                  onSelect={() => {
                    if (deleteAccess.allowed) setConfirmDelete(true);
                  }}
                >
                  {t('flows.edit.delete')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <ChromeButton
              disabled={editAccess.disabled}
              disabledReason={editAccess.reason}
              onClick={() => {
                if (editAccess.allowed) {
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
              disabled={editAccess.disabled || update.isPending}
              disabledReason={!update.isPending ? editAccess.reason : undefined}
            >
              {update.isPending ? t('flows.edit.saving') : t('drawer.save_changes')}
            </ChromeButton>
          </>
        }
      />
      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        destructive
        title={t('flows.edit.delete_confirm_title')}
        description={t('flows.edit.delete_confirm_description')}
        confirmLabel={t('flows.edit.delete_confirm_label')}
        busy={remove.isPending}
        disabled={deleteAccess.disabled}
        disabledReason={deleteAccess.reason}
        onConfirm={() => {
          if (deleteAccess.allowed) remove.mutate();
        }}
      />
      <div className="bg-bg-0 p-3">
        {state ? (
          <QueryState state={state} error={q.error} emptyLabel={t('flows.edit.not_found')} />
        ) : (
          <PipelineForm
            formId="pipeline-form"
            initial={q.data ?? null}
            validationRequest={validationRequest}
            disabled={editAccess.disabled}
            disabledReason={editAccess.reason}
            enabledDisabled={pauseAccess.disabled}
            enabledDisabledReason={pauseAccess.reason}
            onSubmit={(p) => {
              if (editAccess.allowed) update.mutate(p);
            }}
          />
        )}
      </div>
    </>
  );
}
