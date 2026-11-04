import { useMutation, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as notifyApi from '@/api/notify';
import type { NotifyTemplate } from '@/api/notify/templates';
import { toApiError } from '@/lib/http';
import { ReadOnlyValue } from '@/routes/settings/_atoms';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormGroup,
  FormInput,
  FormRadio,
  FormRow,
  FormSection,
  FormSelect,
} from '@/shell/FormDrawer';
import { JsonEditor } from '@/shell/JsonEditor';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { TimeConditionField, windowsInvalid } from '@/shell/weekly-ranges/WeeklyWindowsFields';
import type { TimeCondition } from '@/types/alerting';

import { CONNECTOR_CHANNELS } from '../connector/channels';
import { EVENT_TYPES, NOTIFY_CATEGORIES, policyName } from '../model';
import { PolicyPreview } from './Preview';
import { templateChannelLabel } from '../template/model';

interface DraftState {
  name: string;
  eventType: string;
  category: notifyApi.NotifyCategory;
  priority: string;
  matchers: string;
  resolver: string;
  resolverConfig: string;
  mode: notifyApi.NotifyDeliveryMode;
  connectorIds: string[];
  templateId: string;
  channelTemplates: Partial<Record<notifyApi.NotifyChannel, string>>;
  userFallbacks: boolean;
  teamDefaults: boolean;
  organizationDefaults: boolean;
  ackTimeout: string;
  mergeWindow: string;
  time: TimeCondition | null;
  escalationResolver: string;
  escalationResolverConfig: string;
  escalationMode: notifyApi.NotifyDeliveryMode;
  escalationConnectorIds: string[];
  eventAttributes: string;
  enabled: boolean;
}

function initialDraft(
  policy: notifyApi.NotifyPolicy | null,
  resolvers: string[],
): DraftState {
  const escalation = policy?.escalationConfig as
    | {
        recipientResolver?: string;
        resolverConfig?: unknown;
        deliveryMode?: notifyApi.NotifyDeliveryMode;
        deliveryConfig?: { connectorIds?: string[] };
      }
    | null
    | undefined;
  return {
    name: policy?.name ?? '',
    eventType: policy?.eventType ?? EVENT_TYPES[0],
    category: policy?.category ?? 'alert',
    priority: String(policy?.priority ?? 100),
    matchers: JSON.stringify(policy?.matchers ?? {}, null, 2),
    resolver: policy?.recipientResolver ?? resolvers[0] ?? 'fixed_users',
    resolverConfig: JSON.stringify(policy?.resolverConfig ?? {}, null, 2),
    mode: policy?.deliveryMode ?? 'prefer_user',
    connectorIds: policy?.deliveryConfig.connectorIds ?? [],
    templateId: policy?.templateId ?? '',
    channelTemplates: policy?.channelTemplates ?? {},
    userFallbacks: policy?.fallbackConfig.useUserFallbacks ?? true,
    teamDefaults: policy?.fallbackConfig.useTeamDefaults ?? true,
    organizationDefaults:
      policy?.fallbackConfig.useOrganizationDefaults ?? true,
    ackTimeout: policy?.ackTimeoutSeconds
      ? String(policy.ackTimeoutSeconds)
      : '',
    mergeWindow: policy?.mergeWindowSecs ? String(policy.mergeWindowSecs) : '',
    time: policy?.time ?? null,
    escalationResolver:
      escalation?.recipientResolver ?? resolvers[0] ?? 'fixed_users',
    escalationResolverConfig: JSON.stringify(
      escalation?.resolverConfig ?? {},
      null,
      2,
    ),
    escalationMode: escalation?.deliveryMode ?? 'prefer_user',
    escalationConnectorIds:
      escalation?.deliveryConfig?.connectorIds ?? [],
    eventAttributes: '{}',
    enabled: policy?.enabled ?? true,
  };
}

/** Only alert notifications are merged: of these categories of policy. */
const MERGING_CATEGORIES: notifyApi.NotifyCategory[] = ['alert', 'escalation'];

/** The merge windows offered, in seconds; one set through the API that none matches is listed too. */
function mergeWindowOptions(t: TFunction<'notify'>, current: string) {
  const presets = [30, 60, 120, 300];
  const secs = Number(current);
  const values = current && !presets.includes(secs) ? [...presets, secs].sort((a, b) => a - b) : presets;

  return [
    { value: '', label: t('policies.drawer.merge_off') },
    ...values.map((value) => ({
      value: String(value),
      label:
        value % 60 === 0
          ? t('policies.drawer.merge_minutes', { count: value / 60 })
          : t('policies.drawer.merge_seconds', { count: value }),
    })),
  ];
}

function parseObject(value: string, field: string): Record<string, unknown> {
  const parsed: unknown = JSON.parse(value);
  if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
    throw new Error(`${field} must be a JSON object`);
  }
  return parsed as Record<string, unknown>;
}

function policyInput(draft: DraftState): notifyApi.NotifyPolicyInput {
  const fallback = {
    useUserFallbacks: draft.userFallbacks,
    useTeamDefaults: draft.teamDefaults,
    useOrganizationDefaults: draft.organizationDefaults,
  };
  const ackTimeout = draft.ackTimeout.trim()
    ? Number(draft.ackTimeout)
    : null;
  return {
    name: draft.name.trim(),
    eventType: draft.eventType.trim(),
    category: draft.category,
    priority: Number(draft.priority),
    matchers: parseObject(draft.matchers, 'Matchers'),
    recipientResolver: draft.resolver,
    resolverConfig: parseObject(draft.resolverConfig, 'Resolver config'),
    deliveryMode: draft.mode,
    deliveryConfig: {
      connectorIds:
        draft.mode === 'prefer_user' ? [] : draft.connectorIds,
    },
    templateId: draft.templateId || null,
    channelTemplates: Object.fromEntries(
      Object.entries(draft.channelTemplates).filter(([, id]) => id),
    ),
    fallbackConfig: fallback,
    ackTimeoutSeconds: ackTimeout,
    mergeWindowSecs:
      draft.mergeWindow && MERGING_CATEGORIES.includes(draft.category)
        ? Number(draft.mergeWindow)
        : null,
    time: draft.time,
    escalationConfig:
      ackTimeout === null
        ? null
        : {
            recipientResolver: draft.escalationResolver,
            resolverConfig: parseObject(
              draft.escalationResolverConfig,
              'Escalation resolver config',
            ),
            deliveryMode: draft.escalationMode,
            deliveryConfig: {
              connectorIds:
                draft.escalationMode === 'prefer_user'
                  ? []
                  : draft.escalationConnectorIds,
            },
            fallbackConfig: fallback,
          },
    enabled: draft.enabled,
  };
}

export function PolicyEditor({
  open,
  policy,
  connectors,
  templates,
  resolverTypes,
  onClose,
}: {
  open: boolean;
  policy: notifyApi.NotifyPolicy | null;
  connectors: notifyApi.NotifyConnector[];
  templates: NotifyTemplate[];
  resolverTypes: string[];
  onClose: () => void;
}) {
  const { t } = useTranslation('notify');
  const qc = useQueryClient();
  const [draft, setDraft] = React.useState(() => initialDraft(policy, resolverTypes));
  const [preview, setPreview] =
    React.useState<notifyApi.NotifyPolicyPreview | null>(null);
  const [previewError, setPreviewError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) setDraft(initialDraft(policy, resolverTypes));
  }, [open, policy, resolverTypes]);

  const previewMutation = useMutation({
    mutationFn: (input: {
      policy: notifyApi.NotifyPolicyInput;
      attributes: Record<string, unknown>;
    }) =>
      notifyApi.previewPolicy(input.policy, {
        eventType: input.policy.eventType,
        attributes: input.attributes,
      }),
    onSuccess: (result) => {
      setPreview(result);
      setPreviewError(null);
    },
    onError: (error) => {
      setPreview(null);
      setPreviewError(toApiError(error).message);
    },
  });
  const previewPolicy = previewMutation.mutate;

  React.useEffect(() => {
    if (!open || draft.name.trim() === '') return;
    const timer = window.setTimeout(() => {
      try {
        previewPolicy({
          policy: policyInput(draft),
          attributes: parseObject(draft.eventAttributes, 'Event attributes'),
        });
      } catch (error) {
        setPreview(null);
        setPreviewError(error instanceof Error ? error.message : String(error));
      }
    }, 450);
    return () => window.clearTimeout(timer);
  }, [draft, open, previewPolicy]);

  const save = useMutation({
    mutationFn: (input: notifyApi.NotifyPolicyInput) =>
      policy
        ? notifyApi.updatePolicy(policy.id, input)
        : notifyApi.createPolicy(input),
    onSuccess: () => {
      toast.success(t('common.saved'));
      void qc.invalidateQueries({ queryKey: ['notify', 'policies'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  // A built-in policy goes back to how it is built in, the changes made to it gone.
  const [restoring, setRestoring] = React.useState(false);
  const restore = useMutation({
    mutationFn: (id: string) => notifyApi.restorePolicy(id),
    onSuccess: () => {
      toast.success(t('policies.restored'));
      setRestoring(false);
      void qc.invalidateQueries({ queryKey: ['notify', 'policies'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const set = <K extends keyof DraftState>(key: K, value: DraftState[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));
  // Another category keeps only the templates written for it.
  const setCategory = (category: notifyApi.NotifyCategory) =>
    setDraft((current) => {
      const kept = (id: string | undefined) =>
        templates.some((template) => template.id === id && template.category === category);
      return {
        ...current,
        category,
        templateId: kept(current.templateId) ? current.templateId : '',
        channelTemplates: Object.fromEntries(
          Object.entries(current.channelTemplates).filter(([, id]) => kept(id)),
        ),
      };
    });
  const submit = () => {
    try {
      save.mutate(policyInput(draft));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };
  const selectableConnectors = connectors.filter(
    (connector) => connector.enabled && connector.capabilities.directUser,
  );
  // The templates a slot offers: those of its channel and the policy's category, and the one chosen.
  const templatesFor = (channel: notifyApi.NotifyChannel | null, chosen: string | undefined) =>
    templates.filter(
      (template) =>
        template.channel === channel &&
        (template.category === draft.category || template.id === chosen),
    );
  const generalTemplates = templatesFor(null, draft.templateId);
  const channelSlots = CONNECTOR_CHANNELS.map((channel) => ({
    channel,
    options: templatesFor(channel, draft.channelTemplates[channel]),
  })).filter((slot) => slot.options.length > 0);
  const setChannelTemplate = (channel: notifyApi.NotifyChannel, id: string) =>
    setDraft((current) => ({
      ...current,
      channelTemplates: { ...current.channelTemplates, [channel]: id },
    }));

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      width={1120}
      title={
        policy
          ? t('policies.drawer.edit_title', { name: policyName(t, policy) })
          : t('policies.drawer.new_title')
      }
      subtitle={t('policies.drawer.subtitle')}
      footer={
        <>
          {policy?.builtinKey && (
            <ChromeButton className="mr-auto" onClick={() => setRestoring(true)}>
              {t('policies.drawer.restore')}
            </ChromeButton>
          )}
          <ChromeButton onClick={onClose}>{t('common.cancel')}</ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={draft.name.trim() === '' || (draft.time !== null && windowsInvalid(draft.time)) || save.isPending}
            onClick={submit}
          >
            {save.isPending ? t('common.saving') : t('policies.drawer.save')}
          </ChromeButton>
        </>
      }
    >
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_360px]">
        <div className="min-w-0">
          <FormSection title={t('policies.drawer.trigger')}>
            {policy?.builtinKey ? (
              <FormField label={t('policies.drawer.name')} hint={t('policies.drawer.builtin_name_hint')}>
                <ReadOnlyValue value={policyName(t, policy)} />
              </FormField>
            ) : (
              <FormField label={t('policies.drawer.name')} required>
                <FormInput value={draft.name} onChange={(e) => set('name', e.target.value)} />
              </FormField>
            )}
            <FormRow>
              <FormField label={t('policies.drawer.event_type')} required>
                <FormSelect
                  value={draft.eventType}
                  onChange={(value) => set('eventType', value)}
                  options={EVENT_TYPES.map((value) => ({
                    value,
                    label: t(`event_types.${value}`, {
                      defaultValue: value,
                    }),
                  }))}
                />
              </FormField>
              <FormField label={t('policies.drawer.category')}>
                <FormSelect
                  value={draft.category}
                  onChange={(value) =>
                    setCategory(value as notifyApi.NotifyCategory)
                  }
                  options={NOTIFY_CATEGORIES.map((value) => ({
                    value,
                    label: t(`preferences.${value}`),
                  }))}
                />
              </FormField>
            </FormRow>
            <FormRow>
              <FormField label={t('policies.drawer.priority')}>
                <FormInput
                  type="number"
                  min={0}
                  max={10000}
                  value={draft.priority}
                  onChange={(e) => set('priority', e.target.value)}
                />
              </FormField>
              <label className="flex min-h-9 items-center justify-between gap-4 self-end rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1">
                <span>{t('common.enabled')}</span>
                <Switch checked={draft.enabled} onCheckedChange={(v) => set('enabled', v)} />
              </label>
            </FormRow>
            <FormField label={t('policies.drawer.matchers')}>
              <JsonEditor
                value={draft.matchers}
                onChange={(value) => set('matchers', value)}
                ariaLabel={t('policies.drawer.matchers')}
              />
            </FormField>
            <TimeConditionField
              value={draft.time}
              onChange={(time) => set('time', time)}
              hint={t('policies.drawer.time_hint')}
            />
          </FormSection>

          <FormSection title={t('policies.drawer.recipient')}>
            <FormField label={t('policies.drawer.resolver')}>
              <FormSelect
                value={draft.resolver}
                onChange={(value) => set('resolver', value)}
                options={resolverTypes.map((value) => ({
                  value,
                  label: t(`resolver_types.${value}`, {
                    defaultValue: value,
                  }),
                }))}
              />
            </FormField>
            <FormField label={t('policies.drawer.resolver_config')}>
              <JsonEditor
                value={draft.resolverConfig}
                onChange={(value) => set('resolverConfig', value)}
                ariaLabel={t('policies.drawer.resolver_config')}
              />
            </FormField>
          </FormSection>

          <FormSection title={t('policies.drawer.delivery')}>
            <FormField
              label={t('policies.drawer.template')}
              hint={t('policies.drawer.template_hint')}
            >
              <FormSelect
                value={draft.templateId}
                onChange={(value) => set('templateId', value)}
                options={[
                  {
                    value: '',
                    label: t('policies.drawer.default_template'),
                  },
                  ...generalTemplates.map((template) => ({
                    value: template.id,
                    label: `${template.name} · ${template.format ?? 'text'}`,
                  })),
                ]}
              />
            </FormField>
            {channelSlots.length > 0 && (
              <div data-channel-templates className="flex flex-col gap-1.5">
                <span className={uiLabelClass}>{t('policies.drawer.channel_templates')}</span>
                <div className="grid gap-3 sm:grid-cols-2">
                  {channelSlots.map(({ channel, options }) => (
                    <FormField key={channel} label={templateChannelLabel(t, channel)}>
                      <FormSelect
                        value={draft.channelTemplates[channel] ?? ''}
                        onChange={(value) => setChannelTemplate(channel, value)}
                        options={[
                          { value: '', label: t('policies.drawer.use_general_template') },
                          ...options.map((template) => ({
                            value: template.id,
                            label: template.name,
                          })),
                        ]}
                      />
                    </FormField>
                  ))}
                </div>
              </div>
            )}
            <FormRadio
              value={draft.mode}
              onChange={(value) => set('mode', value)}
              options={(['prefer_user', 'force_connector', 'multi_connector'] as const).map(
                (value) => ({
                  value,
                  label: t(`policies.delivery_modes.${value}`),
                }),
              )}
            />
            {draft.mode !== 'prefer_user' && (
              <FormGroup label={t('policies.drawer.connectors')}>
                <FormChecklist
                  selected={draft.connectorIds}
                  onChange={(value) => set('connectorIds', value)}
                  options={selectableConnectors.map((connector) => ({
                    value: connector.id,
                    label: connector.name,
                    hint: connector.connectorType,
                  }))}
                />
              </FormGroup>
            )}
            {MERGING_CATEGORIES.includes(draft.category) && (
              <FormField label={t('policies.drawer.merge_window')} hint={t('policies.drawer.merge_window_hint')}>
                <FormSelect
                  value={draft.mergeWindow}
                  onChange={(value) => set('mergeWindow', value)}
                  ariaLabel={t('policies.drawer.merge_window')}
                  options={mergeWindowOptions(t, draft.mergeWindow)}
                />
              </FormField>
            )}
          </FormSection>

          <FormSection title={t('policies.drawer.failure')}>
            {(
              [
                ['userFallbacks', 'user_fallbacks'],
                ['teamDefaults', 'team_defaults'],
                ['organizationDefaults', 'organization_defaults'],
              ] as const
            ).map(([key, label]) => (
              <label
                key={key}
                className="flex min-h-11 items-center justify-between gap-4 rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1"
              >
                <span>{t(`policies.drawer.${label}`)}</span>
                <Switch checked={draft[key]} onCheckedChange={(value) => set(key, value)} />
              </label>
            ))}
            <FormField label={t('policies.drawer.ack_timeout')}>
              <FormInput
                type="number"
                min={1}
                placeholder={t('policies.drawer.ack_disabled')}
                value={draft.ackTimeout}
                onChange={(e) => set('ackTimeout', e.target.value)}
              />
            </FormField>
            {draft.ackTimeout !== '' && (
              <>
                <FormField label={t('policies.drawer.escalation_resolver')}>
                  <FormSelect
                    value={draft.escalationResolver}
                    onChange={(value) => set('escalationResolver', value)}
                    options={resolverTypes.map((value) => ({
                      value,
                      label: t(`resolver_types.${value}`, {
                        defaultValue: value,
                      }),
                    }))}
                  />
                </FormField>
                <FormField label={t('policies.drawer.escalation_config')}>
                  <JsonEditor
                    value={draft.escalationResolverConfig}
                    onChange={(value) => set('escalationResolverConfig', value)}
                    ariaLabel={t('policies.drawer.escalation_config')}
                  />
                </FormField>
                <FormField label={t('policies.drawer.escalation_delivery')}>
                  <FormRadio
                    value={draft.escalationMode}
                    onChange={(value) => set('escalationMode', value)}
                    options={(
                      ['prefer_user', 'force_connector', 'multi_connector'] as const
                    ).map((value) => ({
                      value,
                      label: t(`policies.delivery_modes.${value}`),
                    }))}
                  />
                </FormField>
                {draft.escalationMode !== 'prefer_user' && (
                  <FormGroup label={t('policies.drawer.escalation_connectors')}>
                    <FormChecklist
                      selected={draft.escalationConnectorIds}
                      onChange={(value) => set('escalationConnectorIds', value)}
                      options={selectableConnectors.map((connector) => ({
                        value: connector.id,
                        label: connector.name,
                        hint: connector.connectorType,
                      }))}
                    />
                  </FormGroup>
                )}
              </>
            )}
          </FormSection>
          <FormSection title={t('policies.drawer.preview_event')}>
            <FormField label={t('policies.drawer.event_attributes')}>
              <JsonEditor
                value={draft.eventAttributes}
                onChange={(value) => set('eventAttributes', value)}
                ariaLabel={t('policies.drawer.event_attributes')}
              />
            </FormField>
          </FormSection>
        </div>
        <PolicyPreview
          preview={preview}
          error={previewError}
          loading={previewMutation.isPending}
          templates={templates}
        />
      </div>
      <ConfirmDialog
        open={restoring}
        onOpenChange={(next) => !next && setRestoring(false)}
        title={t('policies.restore_title')}
        description={t('policies.restore_description')}
        confirmLabel={t('policies.drawer.restore')}
        cancelLabel={t('common.cancel')}
        busy={restore.isPending}
        onConfirm={() => policy && restore.mutate(policy.id)}
      />
    </FormDrawer>
  );
}
