import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Cable, Mail, Pencil, Trash2, Webhook } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as notifyApi from '@/api/notify';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { ConnectorEditor } from './Editor';
import { formatMicros, statusTone } from '../model';
import { NotifySettingsPage } from '../SettingsPage';

const CAPABILITY_KEYS: Array<keyof notifyApi.ConnectorCapabilities> = [
  'direct_user',
  'group',
  'rich_text',
  'interactive',
  'acknowledgement',
  'attachments',
];

function ConnectorIcon({ type }: { type: string }) {
  if (type === 'email_smtp') return <Mail className="h-4 w-4" />;
  if (type.includes('webhook')) return <Webhook className="h-4 w-4" />;
  return <Cable className="h-4 w-4" />;
}

export function NotifyConnectorsPage() {
  const { t, i18n } = useTranslation('notify');
  const qc = useQueryClient();
  const manage = useActionAccess({ permission: 'alerts.manage' });
  const [editorOpen, setEditorOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<notifyApi.NotifyConnector | null>(null);
  const [removing, setRemoving] = React.useState<notifyApi.NotifyConnector | null>(null);
  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: notifyApi.listConnectors,
  });
  const connectorTypes = useQuery({
    queryKey: ['notify', 'connector-types'],
    queryFn: notifyApi.listConnectorTypes,
  });
  const rows = connectors.data ?? [];
  const state = productStateFor(
    queryStateFor({
      isLoading: connectors.isLoading,
      isError: connectors.isError,
      data: rows,
    }),
    {
      error: connectors.error,
      emptyTitle: t('connectors.empty_title'),
      emptyDescription: t('connectors.empty_description'),
    },
  );
  const remove = useMutation({
    mutationFn: (id: string) => notifyApi.removeConnector(id),
    onSuccess: () => {
      toast.success(t('common.deleted'));
      setRemoving(null);
      void qc.invalidateQueries({ queryKey: ['notify', 'connectors'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const openEditor = (connector: notifyApi.NotifyConnector | null) => {
    setEditing(connector);
    setEditorOpen(true);
  };

  return (
    <>
      <NotifySettingsPage
        title={t('connectors.title')}
        subtitle={t('connectors.subtitle')}
        toolbar={
          <ChromeButton
            variant="primary"
            className="h-11 md:h-9"
            disabled={manage.disabled}
            disabledReason={manage.reason}
            onClick={() => openEditor(null)}
          >
            {t('connectors.new')}
          </ChromeButton>
        }
        state={state}
      >
        <div className="overflow-x-auto rounded-md border border-bd-0 bg-bg-1">
          <DataTable
            rows={rows}
            rowKey={(row) => row.id}
            onRowClick={(row) => manage.allowed && openEditor(row)}
            isRowClickDisabled={() => manage.disabled}
            rowClickDisabledReason={() => manage.reason}
            columns={[
              {
                key: 'name',
                header: t('connectors.columns.name'),
                width: '24%',
                cell: (row) => (
                  <div className="flex min-w-0 items-center gap-3">
                    <span className="grid h-8 w-8 shrink-0 place-items-center rounded-md border border-bd-0 bg-bg-2 text-tx-2">
                      <ConnectorIcon type={row.connector_type} />
                    </span>
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold text-tx-0">{row.name}</div>
                      <div className="truncate font-mono text-xs text-tx-3">{row.id}</div>
                    </div>
                  </div>
                ),
              },
              {
                key: 'type',
                header: t('connectors.columns.type'),
                width: 150,
                cell: (row) => (
                  <Pill tone="blue">
                    {t(`connector_types.${row.connector_type}`, { defaultValue: row.connector_type })}
                  </Pill>
                ),
              },
              {
                key: 'capabilities',
                header: t('connectors.columns.capabilities'),
                width: '30%',
                cell: (row) => (
                  <div className="flex flex-wrap gap-1">
                    {CAPABILITY_KEYS.filter((key) => row.capabilities[key])
                      .slice(0, 3)
                      .map((key) => (
                        <Pill key={key} tone="dim">{t(`capabilities.${key}`)}</Pill>
                      ))}
                  </div>
                ),
              },
              {
                key: 'last_test',
                header: t('connectors.columns.last_test'),
                width: 170,
                cell: (row) => (
                  <span className="text-xs text-tx-2">
                    {formatMicros(row.last_tested_at, i18n.language)}
                  </span>
                ),
              },
              {
                key: 'status',
                header: t('connectors.columns.status'),
                width: 110,
                cell: (row) => (
                  <Pill tone={statusTone(row.status)}>
                    {t(`connectors.status.${row.enabled ? row.status : 'disabled'}`, {
                      defaultValue: row.enabled ? row.status : t('common.disabled'),
                    })}
                  </Pill>
                ),
              },
              {
                key: 'actions',
                header: t('connectors.columns.actions'),
                headerClassName: 'text-right',
                width: 90,
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
                            openEditor(row);
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

      <ConnectorEditor
        open={editorOpen}
        connector={editing}
        connectorTypes={connectorTypes.data ?? []}
        onClose={() => {
          setEditorOpen(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('connectors.delete_title')}
        description={t('connectors.delete_description')}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        busy={remove.isPending}
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </>
  );
}
