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
    permission: 'org.settings.manage',
  });
  const federationAccess = useActionAccess({
    permission: 'org.settings.manage',
    feature: 'federated_search',
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
            : t('nodes.service_graph.ingest_hint')
        }
      >
        <Switch
          checked={storageMode}
          disabled={controlsDisabled}
          disabledReason={controlsDisabledReason}
          onCheckedChange={(checked) => {
            if (!controlsDisabled) {
              update.mutate({ source: checked ? 'storage' : 'ingest' });
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
    setClusterId(query.data.cluster_id);
    setDrain(String(query.data.drain_interval_secs));
    setBatch(String(query.data.push_batch_size));
    setTtl(String(query.data.seen_events_ttl_secs));
    setGossip(String(query.data.gossip_interval_secs));
    hydrated.current = true;
  }, [query.data]);

  const buildSettings = React.useCallback(
    (): instanceApi.FederationSettings => ({
      cluster_id: clusterId.trim(),
      drain_interval_secs: Math.max(1, Number(drain) || 10),
      push_batch_size: Math.max(1, Number(batch) || 100),
      seen_events_ttl_secs: Math.max(1, Number(ttl) || 604800),
      gossip_interval_secs: Math.max(1, Number(gossip) || 60),
    }),
    [batch, clusterId, drain, gossip, ttl],
  );

  const draft = buildSettings();
  const dirty =
    query.data !== undefined &&
    (draft.cluster_id !== query.data.cluster_id ||
      draft.drain_interval_secs !== query.data.drain_interval_secs ||
      draft.push_batch_size !== query.data.push_batch_size ||
      draft.seen_events_ttl_secs !== query.data.seen_events_ttl_secs ||
      draft.gossip_interval_secs !== query.data.gossip_interval_secs);
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
      setClusterId(saved.cluster_id);
      setDrain(String(saved.drain_interval_secs));
      setBatch(String(saved.push_batch_size));
      setTtl(String(saved.seen_events_ttl_secs));
      setGossip(String(saved.gossip_interval_secs));
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
    const disabling = Boolean(query.data?.cluster_id) && !next.cluster_id;
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
    query.data?.cluster_id,
    save,
  ]);

  const reset = React.useCallback(() => {
    if (!query.data) return;
    setClusterId(query.data.cluster_id);
    setDrain(String(query.data.drain_interval_secs));
    setBatch(String(query.data.push_batch_size));
    setTtl(String(query.data.seen_events_ttl_secs));
    setGossip(String(query.data.gossip_interval_secs));
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
