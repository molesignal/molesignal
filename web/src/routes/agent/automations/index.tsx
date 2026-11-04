import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as automationsApi from '@/api/agent/automations';
import type { Automation } from '@/api/agent/automations';
import { toApiError } from '@/lib/http';
import { activeTimeZone } from '@/lib/time';
import { ProductState } from '@/product/states';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';

import { AUTOMATIONS_KEY, AutomationEditor, type EditorOpening } from './AutomationEditor';
import { AutomationTable } from './AutomationTable';
import { draftForClone } from './model';
import { RunsDrawer } from './RunsDrawer';
import { TemplatePicker } from './TemplatePicker';
import { TokenDialog } from './TokenDialog';
import { EmptyTab, ModulePage } from '../operations/Pages';
import { useScopes } from '../scope/useScopes';

/** The list is read again this often, so runs that started on their own show up. */
const REFRESH_MS = 30_000;

/**
 * Automations: Mole Agent doing a job with nobody in the chat — on a
 * schedule, for new incidents, when a CI/CD job calls, or by hand. Each run is
 * a hidden chat the people who see the automation can open.
 */
export function AutomationsPage() {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const scopes = useScopes();
  const timezone = React.useMemo(() => activeTimeZone(), []);
  const list = useQuery({
    queryKey: AUTOMATIONS_KEY,
    queryFn: automationsApi.listAutomationsPage,
    retry: false,
    refetchInterval: REFRESH_MS,
  });
  // Times are told from the moment the list was read.
  const nowMicros = list.dataUpdatedAt * 1000;

  const [picking, setPicking] = React.useState(false);
  const [editing, setEditing] = React.useState<EditorOpening | null>(null);
  const [runsOf, setRunsOf] = React.useState<Automation | null>(null);
  const [tokenOf, setTokenOf] = React.useState<Automation | null>(null);
  const [deleting, setDeleting] = React.useState<Automation | null>(null);
  const [busyId, setBusyId] = React.useState<string | null>(null);

  const refresh = () => void queryClient.invalidateQueries({ queryKey: AUTOMATIONS_KEY });
  const failed = (error: unknown) => toast.error(toApiError(error).message);

  const toggle = useMutation({
    mutationFn: ({ automation, enabled }: { automation: Automation; enabled: boolean }) =>
      automationsApi.setAutomationEnabled(automation.id, enabled),
    onMutate: ({ automation }) => setBusyId(automation.id),
    onSettled: () => {
      setBusyId(null);
      refresh();
    },
    onError: failed,
  });
  const run = useMutation({
    mutationFn: (automation: Automation) => automationsApi.runAutomation(automation.id),
    onMutate: (automation) => setBusyId(automation.id),
    onSuccess: (started, automation) => {
      toast.success(
        started.status === 'running'
          ? t('automations.toast.started', { name: automation.name })
          : t('automations.toast.not_started', {
              name: automation.name,
              reason: t(`automations.reasons.${started.reason ?? 'error'}`, {
                defaultValue: started.reason ?? '',
              }),
            }),
      );
      setRunsOf(automation);
    },
    onSettled: () => {
      setBusyId(null);
      refresh();
    },
    onError: failed,
  });
  const remove = useMutation({
    mutationFn: (automation: Automation) => automationsApi.deleteAutomation(automation.id),
    onSuccess: () => {
      toast.success(t('automations.toast.deleted'));
      setDeleting(null);
      refresh();
    },
    onError: failed,
  });

  const canCreate = list.data?.canCreate ?? false;
  const automations = list.data?.automations ?? [];
  const create = (
    <Button
      size="sm"
      disabled={!canCreate}
      disabledReason={canCreate ? undefined : t('automations.denied')}
      onClick={() => setPicking(true)}
    >
      <Plus />
      {t('automations.create')}
    </Button>
  );

  return (
    <ModulePage
      title={t('automations.title')}
      description={t('automations.description')}
      empty={list.isSuccess && automations.length === 0}
      action={create}
      flush
    >
      {list.isPending ? (
        <ProductState variant="loading" />
      ) : list.isError ? (
        <ProductState variant="error" error={list.error} />
      ) : automations.length === 0 ? (
        <EmptyTab
          title={t('automations.empty_title')}
          purpose={t('automations.description')}
          hint={t('automations.empty_hint')}
          action={create}
        />
      ) : (
        <div className="overflow-x-auto">
          <AutomationTable
            automations={automations}
            nowMicros={nowMicros}
            busyId={busyId}
            scopeLabel={(automation) =>
              scopes.label({
                scope: automation.scope.scope,
                teamId: automation.scope.scope === 'team' ? automation.scope.teamId : null,
                ownerId: automation.scope.scope === 'personal' ? automation.scope.ownerId : null,
              })
            }
            emptyLabel={t('automations.empty_title')}
            onToggle={(automation, enabled) => toggle.mutate({ automation, enabled })}
            onRun={(automation) => run.mutate(automation)}
            onRuns={setRunsOf}
            onEdit={(automation) => setEditing({ automation })}
            onClone={(automation) =>
              setEditing({
                draft: draftForClone(
                  automation,
                  timezone,
                  t('automations.clone_name', { name: automation.name }),
                ),
              })
            }
            onToken={setTokenOf}
            onDelete={setDeleting}
          />
        </div>
      )}

      <TemplatePicker
        open={picking}
        timezone={timezone}
        onClose={() => setPicking(false)}
        onPick={(draft) => {
          setPicking(false);
          setEditing({ draft });
        }}
      />
      <AutomationEditor opening={editing} timezone={timezone} onClose={() => setEditing(null)} />
      <RunsDrawer automation={runsOf} onClose={() => setRunsOf(null)} />
      <TokenDialog automation={tokenOf} onClose={() => setTokenOf(null)} />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={t('automations.delete_confirm.title', { name: deleting?.name ?? '' })}
        description={t('automations.delete_confirm.description')}
        confirmLabel={t('automations.delete_confirm.confirm')}
        cancelLabel={t('actions.cancel', { ns: 'common' })}
        busyLabel={t('automations.delete_confirm.busy')}
        destructive
        busy={remove.isPending}
        onConfirm={() => deleting && remove.mutate(deleting)}
      />
    </ModulePage>
  );
}
