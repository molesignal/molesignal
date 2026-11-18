import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { listConnectors } from '@/api/notify';
import { templateProvider } from '@/api/notify/connectorTypes';
import * as remediationApi from '@/api/remediation';
import type { PreapprovalChannel, RemediationAction } from '@/api/remediation';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, uiLabelClass, uiSectionTitleClass } from '@/shell/chrome';
import { FieldArray, FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { useAuthStore } from '@/stores/auth';

import { ACTIONS_KEY } from './model';
import {
  EXPIRY_CHOICES,
  MAX_CHANNELS,
  MAX_RUNS,
  PREAPPROVAL_KEY,
  type PreapprovalDraft,
  WINDOW_CHOICES,
  approversNeeded,
  preapprovalDraftOf,
  preapprovalInputOf,
  preapprovalStanding,
} from './preapproval';

const STATE_INK: Record<string, string> = {
  none: 'text-tx-3',
  off: 'text-tx-3',
  expired: 'text-tx-3',
  awaiting_approver: 'text-orange',
  used_up: 'text-orange',
  active: 'text-green',
};

/** Pre-approving takes what approving takes. */
function usePreapproveAccess() {
  const preapprove = useActionAccess({ permission: 'alerts.remediation.preapprove' });
  const approve = useActionAccess({ permission: 'agent.approve' });

  return preapprove.disabled ? preapprove : approve;
}

/** The organization's switch: pre-approved actions run without waiting only while it is on. */
export function PreapprovalSwitch() {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'alerts.remediation.preapprove' });
  const overview = useQuery({ queryKey: PREAPPROVAL_KEY, queryFn: remediationApi.preapprovalOverview });

  const switchTo = useMutation({
    mutationFn: (enabled: boolean) => remediationApi.setPreapprovalEnabled(enabled),
    onSuccess: (result) => {
      toast.success(t(result.enabled ? 'remediation.preapproval.switched_on' : 'remediation.preapproval.switched_off'));
      void queryClient.invalidateQueries({ queryKey: PREAPPROVAL_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <section data-remediation-preapproval className="min-w-0">
      <h2 className={`${uiSectionTitleClass} px-[12px] py-2`}>{t('remediation.preapproval.title')}</h2>
      <div className="flex items-start gap-3 bg-[var(--functional-surface)] px-[12px] py-[10px] font-sans">
        <div className="min-w-0 flex-1">
          <div className="text-sm font-strong text-tx-0">{t('remediation.preapproval.switch_label')}</div>
          <p className="mt-0.5 text-xs leading-relaxed text-tx-2">{t('remediation.preapproval.switch_hint')}</p>
        </div>
        <Switch
          checked={overview.data?.enabled ?? false}
          disabled={access.disabled || overview.isLoading || switchTo.isPending}
          aria-label={t('remediation.preapproval.switch_label')}
          onCheckedChange={(enabled) => switchTo.mutate(enabled)}
        />
      </div>
    </section>
  );
}

/** One action's pre-approval: where it stands, and setting or confirming it. */
export function PreapprovalCell({ action }: { action: RemediationAction }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const access = usePreapproveAccess();
  const currentUserId = useAuthStore((state) => state.ctx?.userId ?? '');
  const overview = useQuery({ queryKey: PREAPPROVAL_KEY, queryFn: remediationApi.preapprovalOverview });
  const [editing, setEditing] = React.useState(false);
  const standing = preapprovalStanding(action, overview.data, currentUserId);

  const confirm = useMutation({
    mutationFn: () => remediationApi.confirmPreapproval(action.id),
    onSuccess: () => {
      toast.success(t('remediation.preapproval.confirmed'));
      void queryClient.invalidateQueries({ queryKey: ACTIONS_KEY });
      void queryClient.invalidateQueries({ queryKey: PREAPPROVAL_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <div data-preapproval-state={standing.state} className="flex shrink-0 items-center gap-2">
      <span className={cn('max-w-[260px] truncate text-xs', STATE_INK[standing.state])}>
        {t(`remediation.preapproval.states.${standing.state}`, { used: standing.used, max: standing.max })}
      </span>
      {standing.canConfirm && (
        <ChromeButton
          size="sm"
          disabled={access.disabled || confirm.isPending}
          disabledReason={access.reason}
          onClick={() => confirm.mutate()}
        >
          {t('remediation.preapproval.confirm')}
        </ChromeButton>
      )}
      <ChromeButton size="sm" disabled={access.disabled} disabledReason={access.reason} onClick={() => setEditing(true)}>
        {t(action.preapproval ? 'remediation.preapproval.edit' : 'remediation.preapproval.set')}
      </ChromeButton>
      {editing && <PreapprovalDialog action={action} onClose={() => setEditing(false)} />}
    </div>
  );
}

/** Which incidents the pre-approval covers, how often it may run, until when, and who hears of each run. */
function PreapprovalDialog({ action, onClose }: { action: RemediationAction; onClose: () => void }) {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const { t: tn } = useTranslation('notify');
  const queryClient = useQueryClient();
  const productAccess = useProductAccess();
  const mayListConnectors = hasPermission('notify.connectors.read', productAccess);
  const [now] = React.useState(() => Date.now() * 1000);
  const [draft, setDraft] = React.useState<PreapprovalDraft>(() => preapprovalDraftOf(action, now));
  const set = (patch: Partial<PreapprovalDraft>) => setDraft((current) => ({ ...current, ...patch }));

  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: () => listConnectors(),
    enabled: mayListConnectors,
    retry: false,
  });
  const offered = (connectors.data ?? []).filter(
    (connector) => connector.enabled && templateProvider(connector.connectorType) === null,
  );

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ACTIONS_KEY });
    void queryClient.invalidateQueries({ queryKey: PREAPPROVAL_KEY });
  };

  const save = useMutation({
    mutationFn: () => remediationApi.setPreapproval(action.id, preapprovalInputOf(draft, Date.now() * 1000)),
    onSuccess: () => {
      toast.success(t('remediation.preapproval.saved'));
      refresh();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const clear = useMutation({
    mutationFn: () => remediationApi.clearPreapproval(action.id),
    onSuccess: () => {
      toast.success(t('remediation.preapproval.cleared'));
      refresh();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const firstConnector = offered[0];

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent data-preapproval-dialog>
        <DialogHeader>
          <DialogTitle>{t('remediation.preapproval.dialog_title', { name: action.name })}</DialogTitle>
          <DialogDescription>
            {t(approversNeeded(action.risk) === 2 ? 'remediation.preapproval.dialog_l3' : 'remediation.preapproval.dialog_l2')}
          </DialogDescription>
        </DialogHeader>
        <form
          id="remediation-preapproval-form"
          className="flex min-w-0 flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            save.mutate();
          }}
        >
          <FormField
            label={t('remediation.preapproval.services')}
            hint={t(action.services.length > 0 ? 'remediation.preapproval.services_hint' : 'remediation.preapproval.services_hint_any')}
            required
          >
            <FormInput value={draft.services} onChange={(event) => set({ services: event.target.value })} />
          </FormField>
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
            <FormField label={t('remediation.preapproval.max_runs')} required>
              <FormInput
                type="number"
                min={1}
                max={MAX_RUNS}
                value={String(draft.maxRuns)}
                onChange={(event) => set({ maxRuns: Number(event.target.value) || 1 })}
              />
            </FormField>
            <FormField label={t('remediation.preapproval.window')}>
              <FormSelect
                value={String(draft.windowSecs)}
                onChange={(value) => set({ windowSecs: Number(value) })}
                options={WINDOW_CHOICES.map((secs) => ({
                  value: String(secs),
                  label: t(`remediation.preapproval.windows.${secs}`),
                }))}
              />
            </FormField>
            <FormField label={t('remediation.preapproval.expires')}>
              <FormSelect
                value={draft.expiresInDays === null ? 'never' : String(draft.expiresInDays)}
                onChange={(value) => set({ expiresInDays: value === 'never' ? null : Number(value) })}
                options={EXPIRY_CHOICES.map((days) => ({
                  value: days === null ? 'never' : String(days),
                  label: t(`remediation.preapproval.expiries.${days === null ? 'never' : days}`),
                }))}
              />
            </FormField>
          </div>
          {/* Not a FormField: its label would name the buttons inside. */}
          <div className="flex flex-col gap-1.5">
            <span className={uiLabelClass}>{t('remediation.preapproval.channels')}</span>
            <span className="text-xs text-tx-3">{t('remediation.preapproval.channels_hint')}</span>
            {!mayListConnectors || connectors.isError ? (
              <p className="m-0 text-xs text-tx-3">{t('remediation.preapproval.channels_denied')}</p>
            ) : firstConnector ? (
              <FieldArray<PreapprovalChannel>
                items={draft.channels}
                onChange={(channels) => set({ channels: channels.slice(0, MAX_CHANNELS) })}
                maxItems={MAX_CHANNELS}
                addLabel={t('remediation.preapproval.add_channel')}
                removeLabel={t('remediation.preapproval.remove_channel')}
                newItem={() => ({
                  connectorId: firstConnector.id,
                  targetType: firstConnector.capabilities.group ? 'fixed_group' : 'fixed_address',
                  target: '',
                })}
                renderItem={(channel, index, setChannel) => (
                  <div className="grid grid-cols-[minmax(0,1fr)_120px_minmax(0,1fr)] gap-2">
                    <FormSelect
                      ariaLabel={t('remediation.preapproval.channel_connector', { number: index + 1 })}
                      value={channel.connectorId}
                      onChange={(connectorId) => setChannel({ ...channel, connectorId })}
                      options={offered.map((connector) => ({ value: connector.id, label: connector.name }))}
                    />
                    <FormSelect
                      ariaLabel={t('remediation.preapproval.channel_kind', { number: index + 1 })}
                      value={channel.targetType}
                      onChange={(targetType) =>
                        setChannel({ ...channel, targetType: targetType as PreapprovalChannel['targetType'] })
                      }
                      options={(['fixed_group', 'fixed_address'] as const).map((type) => ({
                        value: type,
                        label: tn(`target_types.${type}`),
                      }))}
                    />
                    <FormInput
                      aria-label={t('remediation.preapproval.channel_target', { number: index + 1 })}
                      value={channel.target}
                      onChange={(event) => setChannel({ ...channel, target: event.target.value })}
                    />
                  </div>
                )}
              />
            ) : (
              <p className="m-0 text-xs text-tx-3">{t('remediation.preapproval.channels_none')}</p>
            )}
          </div>
        </form>
        <DialogFooter>
          {action.preapproval && (
            <ChromeButton
              type="button"
              className="mr-auto enabled:hover:text-red-soft"
              disabled={clear.isPending}
              onClick={() => clear.mutate()}
            >
              {t('remediation.preapproval.clear')}
            </ChromeButton>
          )}
          <ChromeButton type="button" onClick={onClose}>
            {tc('actions.cancel')}
          </ChromeButton>
          <ChromeButton type="submit" form="remediation-preapproval-form" variant="primary" disabled={save.isPending}>
            {t('remediation.preapproval.save')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
