import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as instanceApi from '@/api/instance';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import {
  SettingsGroupStack,
  SettingsRow,
  SettingsSection,
} from '../_atoms';
import { useSettingsSaveStatus } from '../SettingsSaveStatus';

export function DataPlaneRuntimeSettings() {
  const serviceGraphAccess = useActionAccess({
    permission: 'sys.settings.topology.manage',
  });
  const federationAccess = useActionAccess({
    permission: 'sys.settings.topology.manage',
  });

  return (
    <SettingsGroupStack>
      <ServiceGraphSettingsSection access={serviceGraphAccess} />
      <FederationSettingsSection access={federationAccess} />
    </SettingsGroupStack>
  );
}

function ServiceGraphSettingsSection({ access }: { access: ActionAccess }) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const query = useQuery({
    queryKey: ['settings', 'service_graph'],
    queryFn: () => instanceApi.getServiceGraphSettings(),
  });
  const storageMode = query.data?.source === 'storage';

  const update = useMutation({
    mutationFn: (next: instanceApi.ServiceGraphSettings) =>
      instanceApi.updateServiceGraphSettings(next),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (saved) => {
      qc.setQueryData(['settings', 'service_graph'], saved);
      saveStatus.completeSave();
    },
    onError: (error) => {
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });
  const controlsDisabled =
    access.disabled || query.isLoading || query.isError || update.isPending;
  const controlsDisabledReason =
    access.reason ??
    (query.isLoading
      ? tc('access.loading')
      : query.isError
        ? tc('access.page_unavailable')
        : update.isPending
          ? tc('access.operation_pending')
          : undefined);

  return (
    <SettingsSection
      title={t('nodes.service_graph.title')}
      description={t('nodes.service_graph.subtitle')}
    >
      <SettingsRow
        label={t('nodes.service_graph.storage_mode')}
        description={
          storageMode
            ? t('nodes.service_graph.storage_hint')
            : t('nodes.service_graph.intake_hint')
        }
      >
        <Switch
          checked={storageMode}
          disabled={controlsDisabled}
          disabledReason={controlsDisabledReason}
          onCheckedChange={(checked) => {
            if (!controlsDisabled) {
              update.mutate({ source: checked ? 'storage' : 'intake' });
            }
          }}
          aria-label={t('nodes.service_graph.storage_mode')}
        />
      </SettingsRow>
    </SettingsSection>
  );
}

function FederationSettingsSection({ access }: { access: ActionAccess }) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const query = useQuery({
    queryKey: ['settings', 'federation'],
    queryFn: () => instanceApi.getFederationSettings(),
  });

  const [clusterId, setClusterId] = React.useState('');
  const [drain, setDrain] = React.useState('10');
  const [batch, setBatch] = React.useState('100');
  const [ttl, setTtl] = React.useState('604800');
  const [gossip, setGossip] = React.useState('60');
  const [confirmDisable, setConfirmDisable] = React.useState(false);
  const hydrated = React.useRef(false);

  React.useEffect(() => {
    if (!query.data || hydrated.current) return;
    setClusterId(query.data.clusterId);
    setDrain(String(query.data.drainIntervalSecs));
    setBatch(String(query.data.pushBatchSize));
    setTtl(String(query.data.seenEventsTtlSecs));
    setGossip(String(query.data.gossipIntervalSecs));
    hydrated.current = true;
  }, [query.data]);

  const buildSettings = React.useCallback(
    (): instanceApi.FederationSettings => ({
      clusterId: clusterId.trim(),
      drainIntervalSecs: Math.max(1, Number(drain) || 10),
      pushBatchSize: Math.max(1, Number(batch) || 100),
      seenEventsTtlSecs: Math.max(1, Number(ttl) || 604800),
      gossipIntervalSecs: Math.max(1, Number(gossip) || 60),
    }),
    [batch, clusterId, drain, gossip, ttl],
  );

  const draft = buildSettings();
  const dirty =
    query.data !== undefined &&
    (draft.clusterId !== query.data.clusterId ||
      draft.drainIntervalSecs !== query.data.drainIntervalSecs ||
      draft.pushBatchSize !== query.data.pushBatchSize ||
      draft.seenEventsTtlSecs !== query.data.seenEventsTtlSecs ||
      draft.gossipIntervalSecs !== query.data.gossipIntervalSecs);
  const enabled = clusterId.trim().length > 0;
  const numericValues = [drain, batch, ttl, gossip].map(Number);
  const invalid = numericValues.some(
    (value) => !Number.isFinite(value) || value < 1,
  );

  React.useEffect(() => {
    saveStatus.setDraftDirty('nodes.federation', dirty);
  }, [dirty, saveStatus]);

  React.useEffect(
    () => () => saveStatus.setDraftDirty('nodes.federation', false),
    [saveStatus],
  );

  const save = useMutation({
    mutationFn: (next: instanceApi.FederationSettings) =>
      instanceApi.updateFederationSettings(next),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (saved) => {
      qc.setQueryData(['settings', 'federation'], saved);
      setClusterId(saved.clusterId);
      setDrain(String(saved.drainIntervalSecs));
      setBatch(String(saved.pushBatchSize));
      setTtl(String(saved.seenEventsTtlSecs));
      setGossip(String(saved.gossipIntervalSecs));
      saveStatus.completeSave();
    },
    onError: (error) => {
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });
  const controlsDisabled =
    access.disabled || query.isLoading || query.isError || save.isPending;
  const controlsDisabledReason =
    access.reason ??
    (query.isLoading
      ? tc('access.loading')
      : query.isError
        ? tc('access.page_unavailable')
        : save.isPending
          ? tc('access.operation_pending')
          : undefined);

  const persist = React.useCallback(() => {
    if (access.disabled || !dirty || invalid || save.isPending) return;
    const next = buildSettings();
    const disabling = Boolean(query.data?.clusterId) && !next.clusterId;
    if (disabling) {
      setConfirmDisable(true);
      return;
    }
    save.mutate(next);
  }, [
    access.disabled,
    buildSettings,
    dirty,
    invalid,
    query.data?.clusterId,
    save,
  ]);

  const reset = React.useCallback(() => {
    if (!query.data) return;
    setClusterId(query.data.clusterId);
    setDrain(String(query.data.drainIntervalSecs));
    setBatch(String(query.data.pushBatchSize));
    setTtl(String(query.data.seenEventsTtlSecs));
    setGossip(String(query.data.gossipIntervalSecs));
  }, [query.data]);

  const advancedFields = [
    {
      key: 'drain',
      label: t('nodes.federation.fields.drain_interval_secs'),
      value: drain,
      set: setDrain,
    },
    {
      key: 'batch',
      label: t('nodes.federation.fields.push_batch_size'),
      value: batch,
      set: setBatch,
    },
    {
      key: 'ttl',
      label: t('nodes.federation.fields.seen_events_ttl_secs'),
      value: ttl,
      set: setTtl,
    },
    {
      key: 'gossip',
      label: t('nodes.federation.fields.gossip_interval_secs'),
      value: gossip,
      set: setGossip,
    },
  ];

  return (
    <>
      <ConfirmDialog
        open={confirmDisable}
        onOpenChange={setConfirmDisable}
        title={t('nodes.federation.disable_confirm_title')}
        description={t('nodes.federation.disable_confirm_description')}
        confirmLabel={t('nodes.federation.disable_confirm_action')}
        cancelLabel={tc('actions.cancel')}
        busy={save.isPending}
        disabled={access.disabled}
        disabledReason={access.reason}
        onConfirm={() => {
          if (access.disabled) return;
          setConfirmDisable(false);
          save.mutate(buildSettings());
        }}
      />
      <SettingsSection
        title={t('nodes.federation.title')}
        description={t('nodes.federation.subtitle')}
      >
        <SettingsRow
          label={t('nodes.federation.fields.cluster_id')}
          description={t('nodes.federation.hints.cluster_id')}
          controlClassName="w-full"
        >
          <div className="w-full">
            <FormInput
              value={clusterId}
              onChange={(event) => setClusterId(event.target.value)}
              placeholder={t('nodes.federation.placeholder_cluster_id')}
              autoComplete="off"
              disabled={controlsDisabled}
              disabledReason={controlsDisabledReason}
              aria-label={t('nodes.federation.fields.cluster_id')}
            />
            <div className="mt-2 flex items-center gap-2">
              <Pill tone={enabled ? 'green' : 'dim'}>
                {enabled
                  ? t('nodes.federation.status_on')
                  : t('nodes.federation.status_off')}
              </Pill>
            </div>
          </div>
        </SettingsRow>

        <details className="border-b border-bd-0 py-4">
          <summary className="cursor-pointer select-none font-sans text-sm font-strong text-tx-1">
            {t('nodes.federation.advanced')}
          </summary>
          <div className="mt-4 flex max-w-2xl flex-col gap-4">
            {advancedFields.map((field) => (
              <label key={field.key} className="flex flex-col gap-1.5">
                <span className="font-sans text-xs font-strong text-tx-2">
                  {field.label}
                </span>
                <FormInput
                  type="number"
                  min={1}
                  value={field.value}
                  onChange={(event) => field.set(event.target.value)}
                  disabled={controlsDisabled}
                  disabledReason={controlsDisabledReason}
                />
              </label>
            ))}
          </div>
        </details>

        <div className="flex min-h-16 items-center justify-end gap-3 py-3">
          <span className="font-sans text-xs text-tx-3">
            {dirty
              ? t('nodes.federation.unsaved')
              : t('nodes.federation.saved_inline')}
          </span>
          <div className="flex items-center gap-2">
            <ChromeButton
              onClick={reset}
              disabled={access.disabled || !dirty || save.isPending}
              disabledReason={
                access.reason ??
                (!dirty
                  ? tc('access.no_changes')
                  : save.isPending
                    ? tc('access.operation_pending')
                    : undefined)
              }
            >
              {tc('actions.reset')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              onClick={persist}
              disabled={
                access.disabled ||
                !dirty ||
                invalid ||
                query.isLoading ||
                query.isError ||
                save.isPending
              }
              disabledReason={
                access.reason ??
                (invalid
                  ? tc('access.form_invalid')
                  : !dirty
                    ? tc('access.no_changes')
                    : save.isPending
                      ? tc('access.operation_pending')
                      : controlsDisabledReason)
              }
            >
              {save.isPending
                ? t('nodes.federation.saving')
                : t('nodes.federation.save')}
            </ChromeButton>
          </div>
        </div>
      </SettingsSection>
    </>
  );
}
