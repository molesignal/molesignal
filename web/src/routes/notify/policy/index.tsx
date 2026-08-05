import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Pencil, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as notifyApi from '@/api/notify';
import * as notifyTemplatesApi from '@/api/notify/templates';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { connectorName } from '../model';
import { PolicyEditor } from './Editor';
import { NotifySettingsPage } from '../SettingsPage';

export function NotifyPoliciesPage() {
  const { t } = useTranslation('notify');
  const qc = useQueryClient();
  const manage = useActionAccess({ permission: 'alerts.manage' });
  const [editing, setEditing] = React.useState<notifyApi.NotifyPolicy | null>(null);
  const [editorOpen, setEditorOpen] = React.useState(false);
  const [removing, setRemoving] = React.useState<notifyApi.NotifyPolicy | null>(null);
  const policies = useQuery({
    queryKey: ['notify', 'policies'],
    queryFn: notifyApi.listPolicies,
  });
  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: notifyApi.listConnectors,
  });
  const resolvers = useQuery({
    queryKey: ['notify', 'resolver-types'],
    queryFn: notifyApi.listResolverTypes,
  });
  const templates = useQuery({
    queryKey: ['notify', 'templates'],
    queryFn: notifyTemplatesApi.list,
  });
  const rows = policies.data ?? [];
  const state = productStateFor(
    queryStateFor({
      isLoading: policies.isLoading,
      isError: policies.isError,
      data: rows,
    }),
    {
      error: policies.error,
      emptyTitle: t('policies.empty_title'),
      emptyDescription: t('policies.empty_description'),
      emptyAction: (
        <ChromeButton
          variant="primary"
          disabled={manage.disabled}
          disabledReason={manage.reason}
          onClick={() => setEditorOpen(true)}
        >
          <Plus className="h-4 w-4" />
          {t('policies.new')}
        </ChromeButton>
      ),
    },
  );
  const remove = useMutation({
    mutationFn: notifyApi.removePolicy,
    onSuccess: () => {
      toast.success(t('common.deleted'));
      setRemoving(null);
      void qc.invalidateQueries({ queryKey: ['notify', 'policies'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const open = (policy: notifyApi.NotifyPolicy | null) => {
    setEditing(policy);
    setEditorOpen(true);
  };

  return (
    <>
      <NotifySettingsPage
        title={t('policies.title')}
        subtitle={t('policies.subtitle')}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={manage.disabled}
            disabledReason={manage.reason}
            onClick={() => open(null)}
          >
            <Plus className="h-4 w-4" />
            {t('policies.new')}
          </ChromeButton>
        }
        state={state}
      >
        <div className="overflow-x-auto rounded-md border border-bd-0 bg-bg-1">
          <DataTable
            rows={rows}
            rowKey={(row) => row.id}
            onRowClick={(row) => open(row)}
            columns={[
              {
                key: 'name',
                header: t('policies.columns.name'),
                width: '25%',
                cell: (row) => (
                  <div>
                    <div className="truncate text-sm font-semibold text-tx-0">{row.name}</div>
                    <div className="truncate text-xs text-tx-3">
                      {t(`preferences.${row.category}`)} · P
                      {row.priority}
                    </div>
                  </div>
                ),
              },
              {
                key: 'event',
                header: t('policies.columns.event'),
                width: '18%',
                cell: (row) => (
                  <Pill tone="blue">
                    {t(`event_types.${row.event_type}`, {
                      defaultValue: row.event_type,
                    })}
                  </Pill>
                ),
              },
              {
                key: 'resolver',
                header: t('policies.columns.recipient'),
                width: '18%',
                cell: (row) => (
                  <span className="text-xs">
                    {t(`resolver_types.${row.recipient_resolver}`, {
                      defaultValue: row.recipient_resolver,
                    })}
                  </span>
                ),
              },
              {
                key: 'delivery',
                header: t('policies.columns.delivery'),
                width: '20%',
                cell: (row) => (
                  <div className="text-xs text-tx-2">
                    <div>{t(`policies.delivery_modes.${row.delivery_mode}`)}</div>
                    {row.delivery_config.connector_ids.length > 0 && (
                      <div className="truncate text-tx-3">
                        {row.delivery_config.connector_ids
                          .map((id) => connectorName(connectors.data ?? [], id))
                          .join(', ')}
                      </div>
                    )}
                  </div>
                ),
              },
              {
                key: 'fallback',
                header: t('policies.columns.fallback'),
                width: 120,
                cell: (row) => (
                  <span className="text-xs text-tx-2">
                    {[
                      row.fallback_config.use_user_fallbacks && 'U',
                      row.fallback_config.use_team_defaults && 'T',
                      row.fallback_config.use_organization_defaults && 'O',
                    ]
                      .filter(Boolean)
                      .join(' → ') || '—'}
                  </span>
                ),
              },
              {
                key: 'status',
                header: t('policies.columns.status'),
                width: 100,
                cell: (row) => (
                  <Pill tone={row.enabled ? 'green' : 'dim'}>
                    {t(row.enabled ? 'common.enabled' : 'common.disabled')}
                  </Pill>
                ),
              },
              {
                key: 'actions',
                header: '',
                width: 80,
                cell: (row) => (
                  <div className="flex justify-end gap-1">
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <IconButton
                          aria-label={t('common.edit')}
                          disabled={manage.disabled}
                          disabledReason={manage.reason}
                          onClick={(event) => {
                            event.stopPropagation();
                            open(row);
                          }}
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </IconButton>
                      </TooltipTrigger>
                      <TooltipContent>{t('common.edit')}</TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <IconButton
                          aria-label={t('common.delete')}
                          disabled={manage.disabled}
                          disabledReason={manage.reason}
                          onClick={(event) => {
                            event.stopPropagation();
                            setRemoving(row);
                          }}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </IconButton>
                      </TooltipTrigger>
                      <TooltipContent>{t('common.delete')}</TooltipContent>
                    </Tooltip>
                  </div>
                ),
              },
            ]}
          />
        </div>
      </NotifySettingsPage>
      <PolicyEditor
        open={editorOpen}
        policy={editing}
        connectors={connectors.data ?? []}
        templates={templates.data ?? []}
        resolverTypes={resolvers.data ?? []}
        onClose={() => {
          setEditorOpen(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(next) => !next && setRemoving(null)}
        destructive
        title={t('policies.delete_title')}
        description={t('policies.delete_description')}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        busy={remove.isPending}
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </>
  );
}
