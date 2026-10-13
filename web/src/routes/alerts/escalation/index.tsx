import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Pencil, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as escalationsApi from '@/api/escalations';
import * as schedulesApi from '@/api/schedules';
import * as teamsApi from '@/api/teams';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import type { EscalationPolicy } from '@/types/alerting';

import { AlertsPage } from '../Frame';
import { alertFlatTableClassName } from '../Surfaces';
import { EscalationChain } from './Chain';
import { EscalationDrawer } from './Drawer';
import type { TargetNames } from './steps';

/**
 * The escalation policies, each as the path an incident takes while nobody
 * acknowledges it. The list names who is told and how long each step waits,
 * so choosing a policy for a rule does not mean opening every one of them.
 */
export function AlertsEscalations() {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<EscalationPolicy | null>(null);
  const [removing, setRemoving] = React.useState<EscalationPolicy | null>(null);

  const query = useQuery({ queryKey: ['escalation-policies'], queryFn: () => escalationsApi.list() });
  const rows = query.data ?? [];
  // The chain names people, schedules and teams; until a name arrives its id stands in for it.
  const users = useUsers();
  const schedulesQuery = useQuery({ queryKey: ['schedules'], queryFn: () => schedulesApi.list() });
  const teamsQuery = useQuery({ queryKey: ['teams'], queryFn: () => teamsApi.list() });
  const names = React.useMemo<TargetNames>(() => {
    const schedules = new Map((schedulesQuery.data ?? []).map((schedule) => [schedule.id, schedule.name]));
    const teams = new Map((teamsQuery.data ?? []).map((team) => [team.id, team.name]));

    return {
      user: users.nameOf,
      schedule: (id) => schedules.get(id) ?? id,
      team: (id) => teams.get(id) ?? id,
    };
  }, [schedulesQuery.data, teamsQuery.data, users.nameOf]);

  const remove = useMutation({
    mutationFn: (id: string) => escalationsApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void queryClient.invalidateQueries({ queryKey: ['escalation-policies'] });
      setRemoving(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const newPolicy = (
    <ChromeButton
      variant="primary"
      disabled={manageAccess.disabled}
      disabledReason={manageAccess.reason}
      onClick={() => setCreating(true)}
    >
      <Plus className="h-4 w-4" />
      {t('escalations.new_policy')}
    </ChromeButton>
  );
  const state: ProductStateProps | null = query.isLoading
    ? { variant: 'loading' }
    : query.isError
      ? {
          variant: 'error',
          error: query.error,
          action: <ChromeButton onClick={() => void query.refetch()}>{t('center.retry')}</ChromeButton>,
        }
      : rows.length === 0
        ? {
            variant: 'empty',
            title: t('escalations.empty_title'),
            description: t('escalations.empty_description'),
            action: newPolicy,
          }
        : null;

  return (
    <>
      {/* An empty list carries the action itself; the header does not offer it a second time. */}
      <AlertsPage action={state?.variant === 'empty' ? undefined : newPolicy} state={state}>
        <DataTable
          className={alertFlatTableClassName}
          rows={rows}
          rowKey={(policy) => policy.id}
          onRowClick={(policy) => setEditing(policy)}
          isRowClickDisabled={() => manageAccess.disabled}
          rowClickDisabledReason={() => manageAccess.reason}
          columns={[
            {
              key: 'name',
              header: t('escalations.columns.name'),
              width: 260,
              cell: (policy) => <span className="block truncate font-strong text-tx-0">{policy.name}</span>,
            },
            {
              key: 'steps',
              header: t('escalations.columns.steps'),
              cell: (policy) => <EscalationChain policy={policy} names={names} />,
            },
            {
              key: 'actions',
              header: t('escalations.columns.actions'),
              width: 110,
              className: 'overflow-visible text-right',
              headerClassName: 'text-right',
              cell: (policy) => (
                <div className="flex items-center justify-end gap-[2px]">
                  <IconButton
                    aria-label={tc('actions.edit')}
                    title={tc('actions.edit')}
                    disabled={manageAccess.disabled}
                    disabledReason={manageAccess.reason}
                    onClick={(event) => {
                      event.stopPropagation();
                      setEditing(policy);
                    }}
                  >
                    <Pencil className="h-3.5 w-3.5" />
                  </IconButton>
                  <IconButton
                    aria-label={tc('actions.delete')}
                    title={tc('actions.delete')}
                    disabled={manageAccess.disabled}
                    disabledReason={manageAccess.reason}
                    className="enabled:hover:text-red-soft"
                    onClick={(event) => {
                      event.stopPropagation();
                      setRemoving(policy);
                    }}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </IconButton>
                </div>
              ),
            },
          ]}
        />
      </AlertsPage>
      <EscalationDrawer
        open={creating || editing !== null}
        editing={editing}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('escalations.delete_title')}
        description={removing?.name ?? ''}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) remove.mutate(removing.id);
        }}
      />
    </>
  );
}
