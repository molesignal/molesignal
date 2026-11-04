import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as remediationApi from '@/api/remediation';
import type { KubernetesCluster, RemediationAction } from '@/api/remediation';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton, IconButton, uiSectionTitleClass } from '@/shell/chrome';
import { FormField, FormInput, FormTextarea } from '@/shell/FormDrawer';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';

import { AlertsPage } from '../Frame';
import { RemediationActionDrawer } from './ActionDrawer';
import { ACTIONS_KEY, CLUSTERS_KEY, targetSummary } from './model';

/** Remediation actions set up beforehand and run on incidents once approved, and the clusters they act on. */
export function AlertsRemediation() {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.remediation.manage' });
  const [drawer, setDrawer] = React.useState<{ open: boolean; editing: RemediationAction | null }>({ open: false, editing: null });
  const [clusterDialog, setClusterDialog] = React.useState<{ open: boolean; editing: KubernetesCluster | null }>({
    open: false,
    editing: null,
  });
  const [removing, setRemoving] = React.useState<RemediationAction | null>(null);
  const actions = useQuery({ queryKey: ACTIONS_KEY, queryFn: remediationApi.listActions });
  const clusters = useQuery({ queryKey: CLUSTERS_KEY, queryFn: remediationApi.listClusters });
  const clusterName = (id: string) => clusters.data?.find((cluster) => cluster.id === id)?.name;

  const remove = useMutation({
    mutationFn: (id: string) => remediationApi.deleteAction(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      setRemoving(null);
      void queryClient.invalidateQueries({ queryKey: ACTIONS_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const test = useMutation({
    mutationFn: (id: string) => remediationApi.testCluster(id),
    onSuccess: (result) => toast.success(t('remediation.clusters.reached', { version: result.version })),
    onError: (error) => toast.error(toApiError(error).message),
  });

  const failed = [actions, clusters].find((query) => query.isError);
  const state: ProductStateProps | null = actions.isLoading || clusters.isLoading
    ? { variant: 'loading' }
    : failed
      ? { variant: 'error', error: failed.error }
      : null;

  return (
    <>
      <AlertsPage
        action={
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onClick={() => setDrawer({ open: true, editing: null })}
          >
            <Plus className="h-4 w-4" />
            {t('remediation.new_action')}
          </ChromeButton>
        }
        state={state}
        contentSurface={false}
      >
        <div className="flex min-w-0 flex-col gap-[12px]">
          <section data-remediation-actions className="min-w-0">
            <h2 className={`${uiSectionTitleClass} px-[12px] py-2`}>{t('remediation.actions_title')}</h2>
            {(actions.data ?? []).length === 0 ? (
              <p className="bg-[var(--functional-surface)] px-[12px] py-4 font-sans text-sm text-tx-2">{t('remediation.empty')}</p>
            ) : (
              <ul className="flex flex-col gap-px">
                {(actions.data ?? []).map((action) => (
                  <li
                    key={action.id}
                    data-remediation-action={action.id}
                    className="flex flex-wrap items-center gap-x-3 gap-y-1 bg-[var(--functional-surface)] py-[10px] pl-[12px] pr-[20px] font-sans"
                  >
                    <div className="min-w-[220px] flex-1">
                      <button
                        type="button"
                        onClick={() => setDrawer({ open: true, editing: action })}
                        className="block max-w-full truncate text-left text-sm font-strong text-tx-0 underline-offset-4 hover:underline"
                      >
                        {action.name}
                      </button>
                      <div className="mt-0.5 truncate font-mono text-xs text-tx-2">{targetSummary(action, clusterName)}</div>
                    </div>
                    <span className="w-[160px] shrink-0 truncate text-xs text-tx-2">
                      {action.services.length > 0 ? action.services.join(', ') : t('remediation.all_incidents')}
                    </span>
                    <span className="w-[120px] shrink-0 text-xs text-tx-1">{t(`remediation.risks.${action.risk}`)}</span>
                    {!action.enabled && <span className="shrink-0 text-xs text-tx-3">{t('remediation.disabled')}</span>}
                    <IconButton
                      aria-label={t('remediation.delete_named', { name: action.name })}
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      className="enabled:hover:text-red-soft"
                      onClick={() => setRemoving(action)}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </IconButton>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section data-kubernetes-clusters className="min-w-0">
            <div className="flex items-center gap-3 px-[12px] py-2">
              <h2 className={uiSectionTitleClass}>{t('remediation.clusters.title')}</h2>
              <ChromeButton
                size="sm"
                className="ml-auto"
                disabled={manageAccess.disabled}
                disabledReason={manageAccess.reason}
                onClick={() => setClusterDialog({ open: true, editing: null })}
              >
                <Plus className="h-3.5 w-3.5" />
                {t('remediation.clusters.add')}
              </ChromeButton>
            </div>
            {(clusters.data ?? []).length === 0 ? (
              <p className="bg-[var(--functional-surface)] px-[12px] py-4 font-sans text-sm text-tx-2">{t('remediation.clusters.empty')}</p>
            ) : (
              <ul className="flex flex-col gap-px">
                {(clusters.data ?? []).map((cluster) => (
                  <li
                    key={cluster.id}
                    data-kubernetes-cluster={cluster.id}
                    className="flex items-center gap-3 bg-[var(--functional-surface)] py-[10px] pl-[12px] pr-[20px] font-sans"
                  >
                    <div className="min-w-0 flex-1">
                      <button
                        type="button"
                        onClick={() => setClusterDialog({ open: true, editing: cluster })}
                        className="block max-w-full truncate text-left text-sm font-strong text-tx-0 underline-offset-4 hover:underline"
                      >
                        {cluster.name}
                      </button>
                      <div className="mt-0.5 truncate font-mono text-xs text-tx-2">{cluster.apiServer}</div>
                    </div>
                    <ChromeButton
                      size="sm"
                      disabled={test.isPending || manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      onClick={() => test.mutate(cluster.id)}
                    >
                      {t('remediation.clusters.test')}
                    </ChromeButton>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      </AlertsPage>
      <RemediationActionDrawer
        open={drawer.open}
        editing={drawer.editing}
        clusters={clusters.data ?? []}
        onClose={() => setDrawer({ open: false, editing: null })}
      />
      {clusterDialog.open && (
        <ClusterDialog editing={clusterDialog.editing} onClose={() => setClusterDialog({ open: false, editing: null })} />
      )}
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('remediation.delete_title')}
        description={removing?.name ?? ''}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </>
  );
}

/** A Kubernetes cluster: its API server, CA and a ServiceAccount token, kept encrypted and never shown again. */
function ClusterDialog({ editing, onClose }: { editing: KubernetesCluster | null; onClose: () => void }) {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [name, setName] = React.useState(editing?.name ?? '');
  const [apiServer, setApiServer] = React.useState(editing?.apiServer ?? 'https://');
  const [ca, setCa] = React.useState(editing?.caCert ?? '');
  const [token, setToken] = React.useState('');
  const save = useMutation({
    mutationFn: () => {
      const input = { name: name.trim(), apiServer: apiServer.trim(), caCert: ca.trim(), token: token.trim() };
      return editing ? remediationApi.updateCluster(editing.id, input) : remediationApi.createCluster(input);
    },
    onSuccess: () => {
      toast.success(t('remediation.clusters.saved'));
      void queryClient.invalidateQueries({ queryKey: CLUSTERS_KEY });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{editing ? t('remediation.clusters.edit_title') : t('remediation.clusters.add')}</DialogTitle>
          <DialogDescription>{t('remediation.clusters.description')}</DialogDescription>
        </DialogHeader>
        <form
          id="kubernetes-cluster-form"
          className="flex min-w-0 flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            save.mutate();
          }}
        >
          <FormField label={t('remediation.clusters.name')} required>
            <FormInput value={name} onChange={(event) => setName(event.target.value)} />
          </FormField>
          <FormField label={t('remediation.clusters.api_server')} required>
            <FormInput value={apiServer} onChange={(event) => setApiServer(event.target.value)} className="font-mono" />
          </FormField>
          <FormField label={t('remediation.clusters.ca')} hint={t('remediation.clusters.ca_hint')}>
            <FormTextarea value={ca} rows={3} onChange={(event) => setCa(event.target.value)} className="font-mono" />
          </FormField>
          <FormField label={t('remediation.clusters.token')} required={!editing}>
            <FormInput
              type="password"
              value={token}
              onChange={(event) => setToken(event.target.value)}
              placeholder={editing ? t('remediation.drawer.secret_kept') : ''}
            />
          </FormField>
        </form>
        <DialogFooter>
          <ChromeButton type="button" onClick={onClose}>
            {tc('actions.cancel')}
          </ChromeButton>
          <ChromeButton type="submit" form="kubernetes-cluster-form" variant="primary" disabled={save.isPending}>
            {tc('actions.save')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
