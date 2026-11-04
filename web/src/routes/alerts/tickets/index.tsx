import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as ticketsApi from '@/api/tickets';
import type { JiraSettings, MeegleSettings, ServiceNowSettings, TicketConnector } from '@/api/tickets';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { CopyableValue } from '@/routes/settings/_atoms';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

import { AlertsPage } from '../Frame';
import { CONNECTORS_KEY, TicketConnectorDrawer } from './ConnectorDrawer';

/** Where incidents become tickets: connectors to Jira, ServiceNow and Meegle. */
export function AlertsTickets() {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.tickets.manage' });
  const [drawer, setDrawer] = React.useState<{ open: boolean; editing: TicketConnector | null }>({ open: false, editing: null });
  const [removing, setRemoving] = React.useState<TicketConnector | null>(null);
  const connectors = useQuery({ queryKey: CONNECTORS_KEY, queryFn: ticketsApi.listConnectors });

  const remove = useMutation({
    mutationFn: (id: string) => ticketsApi.deleteConnector(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      setRemoving(null);
      void queryClient.invalidateQueries({ queryKey: CONNECTORS_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const test = useMutation({
    mutationFn: (id: string) => ticketsApi.testConnector(id),
    onSuccess: (result) =>
      toast.success(result.detail ? t('tickets.reached_as', { name: result.detail }) : t('tickets.reached')),
    onError: (error) => toast.error(toApiError(error).message),
  });

  const state: ProductStateProps | null = connectors.isLoading
    ? { variant: 'loading' }
    : connectors.isError
      ? { variant: 'error', error: connectors.error }
      : null;

  const summary = (connector: TicketConnector) => {
    if (connector.system === 'jira') {
      const jira = connector.settings as JiraSettings;
      return `${jira.siteUrl} · ${jira.projectKey} · ${jira.issueType}`;
    }

    if (connector.system === 'servicenow') {
      const servicenow = connector.settings as ServiceNowSettings;
      return `${servicenow.instanceUrl} · ${servicenow.table}`;
    }

    const meegle = connector.settings as MeegleSettings;
    return `${t(`tickets.sites.${meegle.site}`)} · ${meegle.projectKey} · ${meegle.workItemTypeKey}`;
  };

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
            {t('tickets.new_connector')}
          </ChromeButton>
        }
        state={state}
        contentSurface={false}
      >
        {(connectors.data ?? []).length === 0 ? (
          <p className="bg-[var(--functional-surface)] px-[12px] py-4 font-sans text-sm text-tx-2">{t('tickets.empty')}</p>
        ) : (
          <ul data-ticket-connectors className="flex flex-col gap-px">
            {(connectors.data ?? []).map((connector) => (
              <li
                key={connector.id}
                data-ticket-connector={connector.id}
                className="flex flex-col gap-2 bg-[var(--functional-surface)] py-[10px] pl-[12px] pr-[20px] font-sans"
              >
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <div className="min-w-[220px] flex-1">
                    <button
                      type="button"
                      onClick={() => setDrawer({ open: true, editing: connector })}
                      className="block max-w-full truncate text-left text-sm font-strong text-tx-0 underline-offset-4 hover:underline"
                    >
                      {connector.name}
                    </button>
                    <div className="mt-0.5 truncate text-xs text-tx-2">
                      {t(`tickets.systems.${connector.system}`)} · {summary(connector)}
                    </div>
                  </div>
                  <span className="shrink-0 text-xs text-tx-1">
                    {connector.auto.enabled ? t('tickets.auto_on') : t('tickets.auto_off')}
                  </span>
                  <ChromeButton
                    size="sm"
                    disabled={test.isPending || manageAccess.disabled}
                    disabledReason={manageAccess.reason}
                    onClick={() => test.mutate(connector.id)}
                  >
                    {t('tickets.test')}
                  </ChromeButton>
                  <IconButton
                    aria-label={t('tickets.delete_named', { name: connector.name })}
                    disabled={manageAccess.disabled}
                    disabledReason={manageAccess.reason}
                    className="enabled:hover:text-red-soft"
                    onClick={() => setRemoving(connector)}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </IconButton>
                </div>
                {connector.webhookPath && connector.secretsSet.includes('webhookSecret') && (
                  <div data-jira-webhook className="flex min-w-0 flex-col gap-1">
                    <span className="text-xs text-tx-2">{t('tickets.webhook_address')}</span>
                    <CopyableValue
                      value={`${window.location.origin}${connector.webhookPath}`}
                      copyLabel={t('tickets.copy_webhook')}
                      copiedLabel={tc('actions.copied')}
                    />
                  </div>
                )}
              </li>
            ))}
          </ul>
        )}
      </AlertsPage>
      <TicketConnectorDrawer open={drawer.open} editing={drawer.editing} onClose={() => setDrawer({ open: false, editing: null })} />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('tickets.delete_title')}
        description={removing?.name ?? ''}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </>
  );
}
