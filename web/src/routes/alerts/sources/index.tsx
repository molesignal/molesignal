import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as alertSourcesApi from '@/api/alertSources';
import type { AlertSource, IssuedAlertSource } from '@/api/alertSources';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';
import { formatRelativeMicros } from '@/time/relative';

import { AlertsPage } from '../Frame';
import { alertFlatTableClassName } from '../Surfaces';
import { ALERT_SOURCES_KEY, SourceDrawer } from './Drawer';
import { usePolicyName } from './policyName';

/**
 * The outside systems that send alerts in: what each one is, how its
 * incidents are escalated, and when it was last heard from.
 *
 * A row opens the source's page, where it is set up and where what it sent
 * can be read. A new source is shown its token once, on that page.
 */
export function AlertsSources() {
  const { t, i18n } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.sources.manage' });
  const [creating, setCreating] = React.useState(false);
  const [removing, setRemoving] = React.useState<AlertSource | null>(null);
  const policyName = usePolicyName();

  const query = useQuery({ queryKey: ALERT_SOURCES_KEY, queryFn: () => alertSourcesApi.list() });
  const rows = React.useMemo(
    () => [...(query.data ?? [])].sort((left, right) => left.name.localeCompare(right.name)),
    [query.data],
  );
  const nowMicros = Date.now() * 1000;

  const remove = useMutation({
    mutationFn: (id: string) => alertSourcesApi.remove(id),
    onSuccess: () => {
      toast.success(t('alert_sources.toast_deleted'));
      void queryClient.invalidateQueries({ queryKey: ALERT_SOURCES_KEY });
      setRemoving(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const created = (source: IssuedAlertSource) =>
    navigate(`/alerts/sources/${encodeURIComponent(source.id)}`, { state: { issued: source } });

  const newSource = (
    <ChromeButton
      variant="primary"
      disabled={manageAccess.disabled}
      disabledReason={manageAccess.reason}
      onClick={() => setCreating(true)}
    >
      <Plus className="h-4 w-4" />
      {t('alert_sources.new_source')}
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
            title: t('alert_sources.empty_title'),
            description: t('alert_sources.empty_description'),
            action: newSource,
          }
        : null;

  return (
    <>
      {/* An empty list carries the action itself; the header does not offer it a second time. */}
      <AlertsPage action={state?.variant === 'empty' ? undefined : newSource} state={state}>
        <DataTable
          className={alertFlatTableClassName}
          rows={rows}
          rowKey={(source) => source.id}
          onRowClick={(source) => navigate(`/alerts/sources/${encodeURIComponent(source.id)}`)}
          columns={[
            {
              key: 'name',
              header: t('alert_sources.columns.name'),
              width: 280,
              cell: (source) => (
                <span className="block truncate font-strong text-tx-0" data-testid="alert-source-row">
                  {source.name}
                </span>
              ),
            },
            {
              key: 'kind',
              header: t('alert_sources.columns.kind'),
              width: 200,
              cell: (source) => (
                <span className="block truncate text-tx-1">{t(`alert_sources.kinds.${source.kind}`)}</span>
              ),
            },
            {
              key: 'policy',
              header: t('alert_sources.columns.policy'),
              cell: (source) => <span className="truncate text-tx-1">{policyName(source)}</span>,
            },
            {
              key: 'last_intake',
              header: t('alert_sources.columns.last_intake'),
              width: 180,
              cell: (source) => (
                <span className="tabular-nums text-tx-1">
                  {source.lastIntakeAt
                    ? formatRelativeMicros(source.lastIntakeAt, i18n.language, nowMicros)
                    : t('alert_sources.never_received')}
                </span>
              ),
            },
            {
              key: 'status',
              header: t('alert_sources.columns.status'),
              width: 120,
              cell: (source) => (
                <span className={cn(source.enabled ? 'text-[var(--green-ink)]' : 'text-tx-2')}>
                  {t(source.enabled ? 'alert_sources.status.on' : 'alert_sources.status.off')}
                </span>
              ),
            },
            {
              key: 'actions',
              header: t('alert_sources.columns.actions'),
              width: 84,
              className: 'overflow-visible text-right',
              headerClassName: 'text-right',
              cell: (source) => (
                <IconButton
                  aria-label={t('alert_sources.delete_named', { name: source.name })}
                  title={tc('actions.delete')}
                  disabled={manageAccess.disabled}
                  disabledReason={manageAccess.reason}
                  className="ml-auto enabled:hover:text-red-soft"
                  onClick={(event) => {
                    event.stopPropagation();
                    setRemoving(source);
                  }}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </IconButton>
              ),
            },
          ]}
        />
      </AlertsPage>
      <SourceDrawer open={creating} editing={null} onClose={() => setCreating(false)} onCreated={created} />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('alert_sources.delete_title')}
        description={t('alert_sources.delete_description', { name: removing?.name ?? '' })}
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
