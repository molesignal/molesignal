import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { MoreHorizontal, Pencil, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as modelProvidersApi from '@/api/agent/modelProviders';
import * as qualityApi from '@/api/agent/quality';
import type { Evaluator } from '@/api/agent/quality';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { IconButton } from '@/shell/chrome';
import { Button } from '@/shell/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { EvaluatorEditor } from './EvaluatorEditor';
import { inputFromDraft, draftFromEvaluator } from './model';
import { ScoringSettingsForm } from './ScoringSettingsForm';
import { QUALITY_KEY } from './state';
import { EmptyTab, ModulePage } from '../operations/Pages';

const HEADER = 'whitespace-nowrap';

/** Which evaluator the editor is open for: a new one, or one being changed. */
type Editing = { evaluator: Evaluator | null } | null;

/**
 * The evaluators that score answers with a model, and the settings of the
 * organization's scoring: the review threshold, the random sample for review,
 * and how a prompt is checked before it becomes the default.
 */
export function QualityEvaluators() {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const [editing, setEditing] = React.useState<Editing>(null);
  const [deleting, setDeleting] = React.useState<Evaluator | null>(null);
  const evaluators = useQuery({
    queryKey: [...QUALITY_KEY, 'evaluators'],
    queryFn: qualityApi.listEvaluators,
    retry: false,
  });
  const providers = useQuery({
    queryKey: ['agent', 'model-providers'],
    queryFn: modelProvidersApi.list,
    retry: false,
  });

  const refresh = () => void queryClient.invalidateQueries({ queryKey: QUALITY_KEY });
  const failed = (error: unknown) => toast.error(toApiError(error).message);

  const toggle = useMutation({
    mutationFn: ({ evaluator, enabled }: { evaluator: Evaluator; enabled: boolean }) =>
      qualityApi.updateEvaluator(evaluator.id, { ...inputFromDraft(draftFromEvaluator(evaluator)), enabled }),
    onSettled: refresh,
    onError: failed,
  });
  const remove = useMutation({
    mutationFn: (evaluator: Evaluator) => qualityApi.deleteEvaluator(evaluator.id),
    onSuccess: () => {
      toast.success(t('quality.evaluators.deleted'));
      setDeleting(null);
      refresh();
    },
    onError: failed,
  });

  const rows = evaluators.data ?? [];
  const create = (
    <Button size="sm" disabled={!providers.isSuccess} onClick={() => setEditing({ evaluator: null })}>
      <Plus />
      {t('quality.evaluators.new')}
    </Button>
  );

  return (
    <ModulePage title={t('quality.views.evaluators')} description={t('quality.evaluators.description')} action={create}>
      <div className="flex flex-col gap-6">
        {evaluators.isPending ? (
          <ProductState variant="loading" />
        ) : evaluators.isError ? (
          <ProductState variant="error" error={evaluators.error} />
        ) : rows.length === 0 ? (
          <EmptyTab title={t('quality.evaluators.none')} hint={t('quality.evaluators.none_hint')} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(row) => row.id}
            emptyLabel={t('quality.evaluators.none')}
            className="rounded-none border-0 bg-transparent"
            columns={[
              {
                key: 'name',
                header: t('quality.columns.name'),
                className: 'w-full max-w-0',
                headerClassName: HEADER,
                cell: (row) => (
                  <div data-quality-evaluator={row.id} className="min-w-0 py-1">
                    <div className="truncate text-tx-0">{row.name}</div>
                    <div className="mt-0.5 truncate text-xs text-tx-2" title={row.rubric}>
                      {row.rubric}
                    </div>
                  </div>
                ),
              },
              {
                key: 'targets',
                header: t('quality.evaluators.targets'),
                width: 200,
                headerClassName: HEADER,
                cell: (row) => (
                  <span className="text-xs text-tx-1">
                    {row.targets.map((target) => t(`quality.sources.${target}`)).join(t('quality.list_separator'))}
                  </span>
                ),
              },
              {
                key: 'sample',
                header: t('quality.evaluators.sample_short'),
                width: 72,
                headerClassName: `${HEADER} text-right`,
                className: 'text-right',
                cell: (row) => <span className="tabular-nums">{row.samplePercent}%</span>,
              },
              {
                key: 'model',
                header: t('quality.evaluators.model'),
                width: 168,
                headerClassName: HEADER,
                cell: (row) => (
                  <span className="truncate text-xs text-tx-2">
                    {row.model ??
                      providers.data?.find((provider) => provider.id === row.providerId)?.defaultModel ??
                      t('quality.evaluators.model_default')}
                  </span>
                ),
              },
              {
                key: 'enabled',
                header: t('quality.evaluators.enabled_short'),
                width: 64,
                headerClassName: HEADER,
                cell: (row) => (
                  <Switch
                    checked={row.enabled}
                    disabled={toggle.isPending}
                    aria-label={t('quality.evaluators.toggle', { name: row.name })}
                    onCheckedChange={(enabled) => toggle.mutate({ evaluator: row, enabled })}
                  />
                ),
              },
              {
                key: 'actions',
                header: <span className="sr-only">{t('quality.columns.actions')}</span>,
                width: 72,
                className: 'overflow-visible text-right',
                cell: (row) => (
                  <div className="flex items-center justify-end gap-[2px]">
                    <IconButton
                      aria-label={t('quality.evaluators.edit_named', { name: row.name })}
                      title={t('quality.evaluators.edit')}
                      onClick={() => setEditing({ evaluator: row })}
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </IconButton>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <IconButton aria-label={t('quality.evaluators.more_named', { name: row.name })}>
                          <MoreHorizontal className="h-3.5 w-3.5" />
                        </IconButton>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end" className="w-40">
                        <DropdownMenuItem onSelect={() => setDeleting(row)} className="text-[var(--red-ink)]">
                          <Trash2 className="h-3.5 w-3.5" />
                          {t('quality.evaluators.delete')}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                ),
              },
            ]}
          />
        )}
        <ScoringSettingsForm />
      </div>
      {editing && providers.isSuccess && (
        <EvaluatorEditor
          key={editing.evaluator?.id ?? 'new'}
          evaluator={editing.evaluator}
          providers={providers.data}
          onClose={() => setEditing(null)}
        />
      )}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={t('quality.evaluators.delete_confirm.title', { name: deleting?.name ?? '' })}
        description={t('quality.evaluators.delete_confirm.description')}
        confirmLabel={t('quality.evaluators.delete')}
        cancelLabel={t('actions.cancel', { ns: 'common' })}
        busyLabel={t('quality.evaluators.delete_confirm.busy')}
        destructive
        busy={remove.isPending}
        onConfirm={() => deleting && remove.mutate(deleting)}
      />
    </ModulePage>
  );
}
