import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as notifyApi from '@/api/notify';
import type { NotifyTemplate } from '@/api/notify/templates';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormInput,
  FormRadio,
  FormRow,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { EVENT_TYPES, NOTIFY_CATEGORIES } from '../model';
import { PolicyPreview } from './Preview';

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
  userFallbacks: boolean;
  teamDefaults: boolean;
  organizationDefaults: boolean;
  ackTimeout: string;
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
  const escalation = policy?.escalation_config as
    | {
        recipient_resolver?: string;
        resolver_config?: unknown;
        delivery_mode?: notifyApi.NotifyDeliveryMode;
        delivery_config?: { connector_ids?: string[] };
      }
    | null
    | undefined;
  return {
    name: policy?.name ?? '',
    eventType: policy?.event_type ?? EVENT_TYPES[0],
    category: policy?.category ?? 'alert',
    priority: String(policy?.priority ?? 100),
    matchers: JSON.stringify(policy?.matchers ?? {}, null, 2),
    resolver: policy?.recipient_resolver ?? resolvers[0] ?? 'fixed_users',
    resolverConfig: JSON.stringify(policy?.resolver_config ?? {}, null, 2),
    mode: policy?.delivery_mode ?? 'prefer_user',
    connectorIds: policy?.delivery_config.connector_ids ?? [],
    templateId: policy?.template_id ?? '',
    userFallbacks: policy?.fallback_config.use_user_fallbacks ?? true,
    teamDefaults: policy?.fallback_config.use_team_defaults ?? true,
    organizationDefaults:
      policy?.fallback_config.use_organization_defaults ?? true,
    ackTimeout: policy?.ack_timeout_seconds
      ? String(policy.ack_timeout_seconds)
      : '',
    escalationResolver:
      escalation?.recipient_resolver ?? resolvers[0] ?? 'fixed_users',
    escalationResolverConfig: JSON.stringify(
      escalation?.resolver_config ?? {},
      null,
      2,
    ),
    escalationMode: escalation?.delivery_mode ?? 'prefer_user',
    escalationConnectorIds:
      escalation?.delivery_config?.connector_ids ?? [],
    eventAttributes: '{}',
    enabled: policy?.enabled ?? true,
  };
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
    use_user_fallbacks: draft.userFallbacks,
    use_team_defaults: draft.teamDefaults,
    use_organization_defaults: draft.organizationDefaults,
  };
  const ackTimeout = draft.ackTimeout.trim()
    ? Number(draft.ackTimeout)
    : null;
  return {
    name: draft.name.trim(),
    event_type: draft.eventType.trim(),
    category: draft.category,
    priority: Number(draft.priority),
    matchers: parseObject(draft.matchers, 'Matchers'),
    recipient_resolver: draft.resolver,
    resolver_config: parseObject(draft.resolverConfig, 'Resolver config'),
    delivery_mode: draft.mode,
    delivery_config: {
      connector_ids:
        draft.mode === 'prefer_user' ? [] : draft.connectorIds,
    },
    template_id: draft.templateId || null,
    fallback_config: fallback,
    ack_timeout_seconds: ackTimeout,
    escalation_config:
      ackTimeout === null
        ? null
        : {
            recipient_resolver: draft.escalationResolver,
            resolver_config: parseObject(
              draft.escalationResolverConfig,
              'Escalation resolver config',
            ),
            delivery_mode: draft.escalationMode,
            delivery_config: {
              connector_ids:
                draft.escalationMode === 'prefer_user'
                  ? []
                  : draft.escalationConnectorIds,
            },
            fallback_config: fallback,
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
        event_type: input.policy.event_type,
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

  const set = <K extends keyof DraftState>(key: K, value: DraftState[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));
  const setCategory = (category: notifyApi.NotifyCategory) =>
    setDraft((current) => ({
      ...current,
      category,
      templateId: templates.some(
        (template) =>
          template.id === current.templateId &&
          template.category === category,
      )
        ? current.templateId
        : '',
    }));
  const submit = () => {
    try {
      save.mutate(policyInput(draft));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };
  const selectableConnectors = connectors.filter(
    (connector) => connector.enabled && connector.capabilities.direct_user,
  );
  const selectableTemplates = templates.filter(
    (template) =>
      template.category === draft.category ||
      template.id === draft.templateId,
  );

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      width={1120}
      title={
        policy
          ? t('policies.drawer.edit_title', { name: policy.name })
          : t('policies.drawer.new_title')
      }
      subtitle={t('policies.drawer.subtitle')}
      footer={
        <>
          <ChromeButton onClick={onClose}>{t('common.cancel')}</ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={draft.name.trim() === '' || save.isPending}
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
            <FormField label={t('policies.drawer.name')} required>
              <FormInput value={draft.name} onChange={(e) => set('name', e.target.value)} />
            </FormField>
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
              <FormTextarea
                className="font-mono text-xs"
                value={draft.matchers}
                onChange={(e) => set('matchers', e.target.value)}
              />
            </FormField>
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
              <FormTextarea
                className="font-mono text-xs"
                value={draft.resolverConfig}
                onChange={(e) => set('resolverConfig', e.target.value)}
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
                  ...selectableTemplates.map((template) => ({
                    value: template.id,
                    label: `${template.name} · ${template.format ?? 'text'}`,
                  })),
                ]}
              />
            </FormField>
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
              <FormField label={t('policies.drawer.connectors')}>
                <FormChecklist
                  selected={draft.connectorIds}
                  onChange={(value) => set('connectorIds', value)}
                  options={selectableConnectors.map((connector) => ({
                    value: connector.id,
                    label: connector.name,
                    hint: connector.connector_type,
                  }))}
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
                  <FormTextarea
                    className="font-mono text-xs"
                    value={draft.escalationResolverConfig}
                    onChange={(e) => set('escalationResolverConfig', e.target.value)}
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
                  <FormField label={t('policies.drawer.escalation_connectors')}>
                    <FormChecklist
                      selected={draft.escalationConnectorIds}
                      onChange={(value) => set('escalationConnectorIds', value)}
                      options={selectableConnectors.map((connector) => ({
                        value: connector.id,
                        label: connector.name,
                        hint: connector.connector_type,
                      }))}
                    />
                  </FormField>
                )}
              </>
            )}
          </FormSection>
          <FormSection title={t('policies.drawer.preview_event')}>
            <FormField label={t('policies.drawer.event_attributes')}>
              <FormTextarea
                className="font-mono text-xs"
                value={draft.eventAttributes}
                onChange={(e) => set('eventAttributes', e.target.value)}
              />
            </FormField>
          </FormSection>
        </div>
        <PolicyPreview
          preview={preview}
          error={previewError}
          loading={previewMutation.isPending}
        />
      </div>
    </FormDrawer>
  );
}
