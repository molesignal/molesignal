import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable, PageHeader } from '@/admin';
import * as clustersApi from '@/api/clusters';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor, useLicenseErrorGate } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { SectionBody, SettingsSection } from '../_atoms';
import { DataPlaneRuntimeSettings } from './DataPlaneSettings';
import {
  CreateClusterDrawer,
  OrgMapDrawer,
  RemoteNodesDrawer,
} from './drawers';

export function Nodes() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [creating, setCreating] = React.useState(false);
  const [removing, setRemoving] = React.useState<clustersApi.RemoteCluster | null>(null);
  const [mapping, setMapping] = React.useState<clustersApi.RemoteCluster | null>(null);
  const [viewingNodes, setViewingNodes] = React.useState<clustersApi.RemoteCluster | null>(null);
  const manageAccess = useActionAccess({
    permission: 'org.settings.manage',
    feature: 'federated_search',
  });
  const readAccess = useActionAccess({
    permission: 'org.settings.read',
    feature: 'federated_search',
  });
  const licenseGate = useLicenseErrorGate();

  const q = useQuery({
    queryKey: ['clusters'],
    queryFn: () => clustersApi.list(),
  });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState =
    (state === 'error' ? licenseGate(q.error, 'features.federated_search') : null) ??
    productStateFor(state, {
      error: q.error,
      emptyTitle: t('nodes.empty_title'),
      emptyDescription: t('nodes.empty_description'),
    });

  const remove = useMutation({
    mutationFn: (id: string) => clustersApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void qc.invalidateQueries({ queryKey: ['clusters'] });
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <PageHeader
        title={t('nodes.title')}
        subtitle={t('nodes.subtitle') as string}
        actions={
          <ChromeButton
            variant="primary"
            onClick={() => {
              if (manageAccess.allowed) setCreating(true);
            }}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            {t('nodes.new_cluster')}
          </ChromeButton>
        }
      />
      <CreateClusterDrawer
        open={creating}
        access={manageAccess}
        onClose={() => setCreating(false)}
      />
      <OrgMapDrawer
        cluster={mapping}
        access={manageAccess}
        onClose={() => setMapping(null)}
      />
      <RemoteNodesDrawer
        cluster={viewingNodes}
        onClose={() => setViewingNodes(null)}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('nodes.delete_confirm_title')}
        description={t('nodes.delete_confirm_description')}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) {
            remove.mutate(removing.id);
          }
        }}
      />
      <SectionBody className="space-y-8 pb-10">
        <DataPlaneRuntimeSettings />
        <SettingsSection
          title={t('nodes.remote_clusters_title')}
          description={t('nodes.remote_clusters_description')}
        >
          <div className="py-4">
            {pageState ? (
              <ProductState {...pageState} />
            ) : (
              <DataTable
                rows={rows}
                rowKey={(r) => r.id}
                columns={[
                  { key: 'name', header: t('nodes.columns.name'), cell: (r) => r.name },
                  {
                    key: 'addr',
                    header: t('nodes.columns.address'),
                    cell: (r) => <span className="font-sans text-tx-2">{r.advertise_addr}</span>,
                  },
                  {
                    key: 'tls',
                    header: t('nodes.columns.tls'),
                    cell: (r) =>
                      r.tls_verify ? (
                        <Pill tone="green">{t('nodes.tls_verify_on')}</Pill>
                      ) : (
                        <Pill tone="dim">{tc('status.off')}</Pill>
                      ),
                    width: 100,
                  },
                  {
                    key: 'enabled',
                    header: t('nodes.columns.enabled'),
                    cell: (r) =>
                      r.discovered ? (
                        <Pill tone="yellow">{t('nodes.discovered_badge')}</Pill>
                      ) : r.enabled ? (
                        <Pill tone="green">{tc('status.on')}</Pill>
                      ) : (
                        <Pill tone="dim">{tc('status.off')}</Pill>
                      ),
                    width: 110,
                  },
                  {
                    key: 'actions',
                    header: '',
                    width: 210,
                    cell: (r) => (
                      <div
                        className="flex items-center justify-end gap-1"
                        onClick={(event) => event.stopPropagation()}
                      >
                        <ChromeButton
                          variant="ghost"
                          size="sm"
                          disabled={readAccess.disabled}
                          disabledReason={readAccess.reason}
                          onClick={(e) => {
                            e.stopPropagation();
                            setViewingNodes(r);
                          }}
                        >
                          {t('nodes.nodes_action')}
                        </ChromeButton>
                        <ChromeButton
                          variant="ghost"
                          size="sm"
                          disabled={manageAccess.disabled}
                          disabledReason={manageAccess.reason}
                          onClick={(e) => {
                            e.stopPropagation();
                            if (manageAccess.allowed) setMapping(r);
                          }}
                        >
                          {t('nodes.org_map_action')}
                        </ChromeButton>
                        <ChromeButton
                          variant="ghost"
                          size="sm"
                          disabled={manageAccess.disabled}
                          disabledReason={manageAccess.reason}
                          onClick={(e) => {
                            e.stopPropagation();
                            if (manageAccess.allowed) setRemoving(r);
                          }}
                          className="enabled:hover:text-red-soft"
                        >
                          {tc('actions.delete')}
                        </ChromeButton>
                      </div>
                    ),
                  },
                ]}
              />
            )}
            <p className="mt-3 font-sans text-xs text-tx-2">{t('nodes.license_note')}</p>
          </div>
        </SettingsSection>
      </SectionBody>
    </>
  );
}
