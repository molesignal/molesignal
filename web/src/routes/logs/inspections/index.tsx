import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as providersApi from '@/api/agent/modelProviders';
import * as inspectionsApi from '@/api/logInspections';
import type { LogInspection } from '@/api/logInspections';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton, Dot } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

import { canManageInspections, canReadInspections } from './access';
import { INSPECTIONS_KEY, InspectionEditor } from './Editor';
import { InspectionTable } from './InspectionTable';
import {
  ACTIVE_REFRESH_MS,
  DEFAULT_LIMITS,
  atCapacity,
  hasActiveRun,
  refreshInterval,
} from './model';

/**
 * Scheduled log inspections: Mole Agent inspects a stream on a schedule, keeps
 * each report, and tells the chosen channels when a report says something is
 * wrong. The page lists what is saved; an inspection's reports are on its own
 * page.
 *
 * A run takes a minute or so, so while one waits or is under way the list
 * reads itself again every few seconds.
 */
export function LogInspections() {
  const { t } = useTranslation('logs');
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const readable = canReadInspections(access);
  const manageable = canManageInspections(access);
  const [editing, setEditing] = React.useState<{ inspection: LogInspection | null } | null>(null);
  const [deleting, setDeleting] = React.useState<LogInspection | null>(null);

  // The list and the limits are read together: the page says how many may be saved beside
  // how many are.
  const query = useQuery({
    queryKey: [...INSPECTIONS_KEY, 'list'],
    queryFn: async () => {
      const [inspections, limits] = await Promise.all([
        inspectionsApi.list(),
        inspectionsApi.limits(),
      ]);

      return { inspections, limits };
    },
    enabled: readable,
    staleTime: 0,
    refetchOnWindowFocus: true,
    refetchInterval: (current) => refreshInterval(hasActiveRun(current.state.data?.inspections)),
  });
  const providers = useQuery({
    queryKey: ['agent', 'model-providers'],
    queryFn: () => providersApi.list(),
    enabled: readable,
    retry: false,
  });
  const inspections = query.data?.inspections ?? [];
  const limits = query.data?.limits ?? DEFAULT_LIMITS;
  const noModel = providers.isSuccess && !providers.data.some((provider) => provider.enabled);
  const full = atCapacity(inspections.length, limits);

  const refresh = () => queryClient.invalidateQueries({ queryKey: INSPECTIONS_KEY });
  const onError = (error: unknown) => toast.error(toApiError(error).message);
  const toggle = useMutation({
    mutationFn: ({ inspection, enabled }: { inspection: LogInspection; enabled: boolean }) =>
      inspectionsApi.setEnabled(inspection.id, enabled),
    onSuccess: (saved) => {
      toast.success(t(saved.enabled ? 'inspections.toast.enabled' : 'inspections.toast.paused'));
      return refresh();
    },
    onError,
  });
  const run = useMutation({
    mutationFn: ({ inspection }: { inspection: LogInspection }) =>
      inspectionsApi.runNow(inspection.id),
    onSuccess: () => {
      toast.success(t('inspections.toast.queued'));
      return refresh();
    },
    onError,
  });
  const remove = useMutation({
    mutationFn: ({ inspection }: { inspection: LogInspection }) =>
      inspectionsApi.remove(inspection.id),
    onSuccess: () => {
      toast.success(t('inspections.toast.deleted'));
      setDeleting(null);
      return refresh();
    },
    onError,
  });
  const busyId =
    [toggle, run, remove].find((mutation) => mutation.isPending)?.variables?.inspection.id ?? null;

  const deniedReason = manageable ? undefined : t('inspections.denied.manage');
  const createBlocked = deniedReason
    ?? (noModel ? t('inspections.no_model.short') : undefined)
    ?? (full ? t('inspections.capacity.full', { max: limits.max_per_org }) : undefined);
  const create = (
    <ChromeButton
      variant="primary"
      disabled={Boolean(createBlocked)}
      disabledReason={createBlocked}
      onClick={() => setEditing({ inspection: null })}
      data-log-inspection-new
    >
      <Plus className="h-3.5 w-3.5" aria-hidden="true" />
      {t('inspections.new')}
    </ChromeButton>
  );

  const state: ProductStateProps | null = !readable
    ? {
        variant: 'permission-denied',
        title: t('inspections.denied.title'),
        description: t('inspections.denied.description'),
      }
    : query.isError
      ? { variant: 'error', error: query.error }
      : !query.data
        ? { variant: 'loading' }
        : inspections.length === 0
          ? {
              variant: 'empty',
              title: t('inspections.empty.title'),
              description: t('inspections.empty.description'),
              action: create,
            }
          : null;

  return (
    <>
      <ListPage
        appearance="surface"
        title={t('inspections.title')}
        subtitle={t('inspections.subtitle')}
        toolbar={inspections.length > 0 ? create : undefined}
        state={state}
        lead={
          readable && noModel ? (
            <NoModelNotice canConfigure={hasPermission('agent.manage', access)} />
          ) : undefined
        }
        listHead={
          inspections.length > 0 ? (
            <>
              <span data-log-inspection-count className="font-sans text-xs text-tx-2">
                {t('inspections.capacity.count', {
                  saved: inspections.length,
                  max: limits.max_per_org,
                })}
              </span>
              {hasActiveRun(inspections) && (
                <span
                  data-log-inspections-live
                  className="ml-auto flex items-center gap-[6px] whitespace-nowrap font-sans text-xs text-tx-2"
                >
                  <Dot tone="blue" />
                  {t('inspections.live', { seconds: ACTIVE_REFRESH_MS / 1000 })}
                </span>
              )}
            </>
          ) : undefined
        }
      >
        <InspectionTable
          inspections={inspections}
          nowMicros={Date.now() * 1000}
          busyId={busyId}
          deniedReason={deniedReason}
          emptyLabel={t('inspections.empty.title')}
          onToggle={(inspection, enabled) => toggle.mutate({ inspection, enabled })}
          onRun={(inspection) => run.mutate({ inspection })}
          onEdit={(inspection) => setEditing({ inspection })}
          onDelete={setDeleting}
        />
      </ListPage>

      <InspectionEditor
        open={editing !== null}
        onOpenChange={(open) => !open && setEditing(null)}
        inspection={editing?.inspection ?? null}
        limits={limits}
      />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={t('inspections.delete_confirm.title', { name: deleting?.name ?? '' })}
        description={t('inspections.delete_confirm.description')}
        confirmLabel={t('inspections.delete_confirm.confirm')}
        cancelLabel={t('actions.cancel', { ns: 'common' })}
        busyLabel={t('inspections.delete_confirm.busy')}
        destructive
        busy={remove.isPending}
        onConfirm={() => deleting && remove.mutate({ inspection: deleting })}
      />
    </>
  );
}

/**
 * Said above the list when no model is switched on: the inspections are still
 * there to be read, and every one of them will fail until a model is.
 */
export function NoModelNotice({ canConfigure }: { canConfigure: boolean }) {
  const { t } = useTranslation('logs');

  return (
    <p
      role="status"
      data-log-inspections-no-model
      className="m-0 flex flex-wrap items-center gap-x-[12px] gap-y-[4px] rounded-md bg-[var(--yellow-tint)] px-[12px] py-[8px] font-sans text-xs leading-relaxed text-[var(--yellow-ink)]"
    >
      <span className="min-w-0 flex-1">
        {t('inspections.no_model.notice')}{' '}
        {!canConfigure && t('explore.inspection.unavailable.ask_admin')}
      </span>
      {canConfigure && (
        <Link
          to="/agent/settings/models"
          className="shrink-0 rounded-sm font-strong underline-offset-4 hover:underline focus-visible:underline focus-visible:outline-none"
        >
          {t('explore.inspection.unavailable.configure')}
        </Link>
      )}
    </p>
  );
}
