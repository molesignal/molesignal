import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as intelligenceApi from '@/api/intelligence';
import * as providersApi from '@/api/intelligence/modelProviders';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { parseDelimitedList, stringListValue } from '../editorModel';
import { IntelligenceToolChecklist } from '../IntelligenceToolChecklist';

export type ProfileEditorSection =
  | 'profile'
  | 'tools'
  | 'data'
  | 'network'
  | 'approvals';
export type ProfileEditorTarget = {
  profile: intelligenceApi.AgentProfile | 'new';
  section: ProfileEditorSection;
} | null;
export type ProviderEditorTarget = providersApi.ModelProvider | 'new' | null;

interface ProfileDraft {
  name: string;
  description: string;
  providerId: string;
  model: string;
  allowedTools: string[];
  environments: string;
  services: string;
  streams: string;
  networkAccess: intelligenceApi.NetworkAccess;
  maxContextTokens: string;
  maxInvestigationMinutes: string;
  maxToolCalls: string;
  l0Policy: string;
  l1Policy: string;
  l2Policy: string;
  l3Policy: string;
  isDefault: boolean;
  enabled: boolean;
}

function profileDraft(
  target: intelligenceApi.AgentProfile | 'new',
  existingCount: number,
  toolNames: string[],
): ProfileDraft {
  if (target === 'new') {
    return {
      name: '',
      description: '',
      providerId: '',
      model: '',
      allowedTools: toolNames,
      environments: 'development, staging, production',
      services: '',
      streams: '',
      networkAccess: 'blocked',
      maxContextTokens: '32000',
      maxInvestigationMinutes: '30',
      maxToolCalls: '32',
      l0Policy: 'automatic',
      l1Policy: 'automatic',
      l2Policy: 'approval',
      l3Policy: 'two_person_approval',
      isDefault: existingCount === 0,
      enabled: true,
    };
  }
  return {
    name: target.name,
    description: target.description,
    providerId: target.model_provider_id ?? '',
    model: target.model ?? '',
    allowedTools: target.allowed_tools,
    environments: stringListValue(target.data_scope.environments),
    services: stringListValue(target.data_scope.services),
    streams: stringListValue(target.data_scope.streams),
    networkAccess: target.network_access,
    maxContextTokens: String(target.max_context_tokens),
    maxInvestigationMinutes: String(Math.max(1, Math.round(target.max_investigation_secs / 60))),
    maxToolCalls: String(target.max_tool_calls),
    l0Policy: String(target.risk_policy.l0 ?? 'automatic'),
    l1Policy: String(target.risk_policy.l1 ?? 'automatic'),
    l2Policy: String(target.risk_policy.l2 ?? 'approval'),
    l3Policy: String(target.risk_policy.l3 ?? 'two_person_approval'),
    isDefault: target.is_default,
    enabled: target.enabled,
  };
}

export function ProfileEditorDrawer({
  target,
  profiles,
  providers,
  tools,
  onClose,
}: {
  target: ProfileEditorTarget;
  profiles: intelligenceApi.AgentProfile[];
  providers: providersApi.ModelProvider[];
  tools: intelligenceApi.RegisteredTool[];
  onClose: () => void;
}) {
  const { t } = useTranslation('intelligence');
  const queryClient = useQueryClient();
  const toolNamesKey = tools.map((tool) => tool.name).join('\n');
  const [draft, setDraft] = React.useState<ProfileDraft>(() =>
    profileDraft('new', profiles.length, []),
  );
  const [formError, setFormError] = React.useState('');

  React.useEffect(() => {
    if (!target) return;
    setDraft(
      profileDraft(
        target.profile,
        profiles.length,
        toolNamesKey ? toolNamesKey.split('\n') : [],
      ),
    );
    setFormError('');
  }, [profiles.length, target, toolNamesKey]);

  const save = useMutation({
    mutationFn: ({
      id,
      input,
    }: {
      id: string | null;
      input: intelligenceApi.AgentProfileInput;
      section: ProfileEditorSection;
    }) =>
      id
        ? intelligenceApi.updateProfile(id, input)
        : intelligenceApi.createProfile(input),
    onSuccess: async (_saved, variables) => {
      await queryClient.invalidateQueries({ queryKey: ['intelligence', 'profiles'] });
      onClose();
      toast.success(
        t(
          variables.id
            ? sectionUpdatedKey(variables.section)
            : 'settings.profiles.created',
        ),
      );
    },
    onError: (error) => toast.error(String(error)),
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!target || !draft.name.trim()) return;
    const maxContextTokens = Number(draft.maxContextTokens);
    const maxInvestigationMinutes = Number(draft.maxInvestigationMinutes);
    const maxToolCalls = Number(draft.maxToolCalls);
    if (
      !Number.isFinite(maxContextTokens) ||
      maxContextTokens < 1 ||
      !Number.isFinite(maxInvestigationMinutes) ||
      maxInvestigationMinutes < 1 ||
      !Number.isFinite(maxToolCalls) ||
      maxToolCalls < 1 ||
      maxToolCalls > 256
    ) {
      setFormError(t('settings.profiles.invalid_limits'));
      return;
    }
    setFormError('');
    save.mutate({
      id: target.profile === 'new' ? null : target.profile.id,
      section: target.section,
      input: {
        name: draft.name.trim(),
        description:
          draft.description.trim() || t('settings.profiles.default_description'),
        model_provider_id: draft.providerId || null,
        model: draft.model.trim() || null,
        allowed_tools: draft.allowedTools,
        data_scope: {
          environments: parseDelimitedList(draft.environments),
          services: parseDelimitedList(draft.services),
          streams: parseDelimitedList(draft.streams),
          cross_organization: false,
        },
        risk_policy: {
          l0: draft.l0Policy,
          l1: draft.l1Policy,
          l2: draft.l2Policy,
          l3: draft.l3Policy,
        },
        network_access: draft.networkAccess,
        max_context_tokens: Math.round(maxContextTokens),
        max_investigation_secs: Math.round(maxInvestigationMinutes * 60),
        max_tool_calls: Math.round(maxToolCalls),
        is_default: draft.isDefault,
        enabled: draft.enabled,
      },
    });
  };

  const section = target?.section ?? 'profile';
  const isNew = target?.profile === 'new';
  const titleKey = isNew
    ? 'settings.profiles.create'
    : section === 'profile'
      ? 'settings.profiles.edit'
      : section === 'tools'
        ? 'settings.tools.editor_title'
        : section === 'data'
          ? 'settings.data.editor_title'
          : section === 'network'
            ? 'settings.network.editor_title'
            : 'settings.approval_policy.editor_title';
  const subtitleKey =
    section === 'profile'
      ? 'settings.profiles.editor_description'
      : section === 'tools'
        ? 'settings.tools.editor_description'
        : section === 'data'
          ? 'settings.data.editor_description'
          : section === 'network'
            ? 'settings.network.editor_description'
            : 'settings.approval_policy.editor_description';
  const formId = `intelligence-profile-${section}-editor`;
  const riskOptions = [
    { value: 'automatic', label: t('settings.profiles.policies.automatic') },
    { value: 'approval', label: t('settings.profiles.policies.approval') },
    {
      value: 'two_person_approval',
      label: t('settings.profiles.policies.two_person_approval'),
    },
  ];

  return (
    <FormDrawer
      open={target !== null}
      onOpenChange={(open) => {
        if (!open && !save.isPending) onClose();
      }}
      title={t(titleKey)}
      subtitle={t(subtitleKey)}
      width={section === 'profile' ? 720 : 640}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          onCancel={onClose}
          formId={formId}
          submitLabel={t(isNew ? 'common.create' : 'common.save')}
        />
      }
    >
      <form id={formId} onSubmit={submit}>
        {section === 'profile' && (
          <FormSection title={t('settings.profiles.sections.identity')}>
          <FormField label={t('settings.profiles.fields.name')} required>
            <FormInput
              value={draft.name}
              onChange={(event) => setDraft((value) => ({ ...value, name: event.target.value }))}
              placeholder={t('settings.profiles.name_placeholder')}
              autoFocus
            />
          </FormField>
          <FormField label={t('settings.profiles.fields.description')}>
            <FormTextarea
              value={draft.description}
              onChange={(event) =>
                setDraft((value) => ({ ...value, description: event.target.value }))
              }
            />
          </FormField>
          <FormRow>
            <FormField label={t('settings.profiles.fields.provider')}>
              <FormSelect
                value={draft.providerId}
                onChange={(providerId) => setDraft((value) => ({ ...value, providerId }))}
                options={[
                  { value: '', label: t('settings.profiles.provider_auto') },
                  ...providers.map((provider) => ({
                    value: provider.id,
                    label: provider.name,
                  })),
                ]}
              />
            </FormField>
            <FormField label={t('settings.profiles.fields.model')}>
              <FormInput
                value={draft.model}
                onChange={(event) =>
                  setDraft((value) => ({ ...value, model: event.target.value }))
                }
                placeholder={t('settings.profiles.model_inherit')}
              />
            </FormField>
          </FormRow>
          <div className="grid gap-2 sm:grid-cols-2">
            <SwitchRow
              label={t('settings.profiles.fields.enabled')}
              checked={draft.enabled}
              onCheckedChange={(enabled) => setDraft((value) => ({ ...value, enabled }))}
            />
            <SwitchRow
              label={t('settings.profiles.fields.default')}
              checked={draft.isDefault}
              onCheckedChange={(isDefault) =>
                setDraft((value) => ({ ...value, isDefault }))
              }
            />
          </div>
          </FormSection>
        )}

        {section === 'tools' && (
          <FormSection
            title={t('settings.profiles.sections.tools')}
            description={t('settings.profiles.sections.tools_description')}
          >
            <IntelligenceToolChecklist
              options={tools.map((tool) => ({
                value: tool.name,
                label: tool.name,
                hint: tool.description,
              }))}
              selected={draft.allowedTools}
              onChange={(allowedTools) =>
                setDraft((value) => ({ ...value, allowedTools }))
              }
            />
          </FormSection>
        )}

        {section === 'data' && (
          <FormSection title={t('settings.profiles.sections.data')}>
            <FormField
              label={t('settings.data.environments')}
              hint={t('settings.profiles.list_hint')}
            >
              <FormInput
                value={draft.environments}
                onChange={(event) =>
                  setDraft((value) => ({
                    ...value,
                    environments: event.target.value,
                  }))
                }
              />
            </FormField>
            <FormField
              label={t('settings.data.services')}
              hint={t('settings.profiles.empty_means_all')}
            >
              <FormInput
                value={draft.services}
                onChange={(event) =>
                  setDraft((value) => ({
                    ...value,
                    services: event.target.value,
                  }))
                }
              />
            </FormField>
            <FormField
              label={t('settings.data.streams')}
              hint={t('settings.profiles.empty_means_all')}
            >
              <FormInput
                value={draft.streams}
                onChange={(event) =>
                  setDraft((value) => ({
                    ...value,
                    streams: event.target.value,
                  }))
                }
              />
            </FormField>
          </FormSection>
        )}

        {section === 'network' && (
          <FormSection
            title={t('settings.network.access')}
            description={t('settings.network.editor_help')}
          >
            <SwitchRow
              label={t('settings.network.allow_network_access')}
              checked={draft.networkAccess === 'allowed'}
              onCheckedChange={(allowed) =>
                setDraft((value) => ({
                  ...value,
                  networkAccess: allowed ? 'allowed' : 'blocked',
                }))
              }
            />
            <div
              className={
                draft.networkAccess === 'allowed'
                  ? 'rounded-md border border-yellow/30 bg-yellow-dim px-3 py-2 text-sm leading-6 text-yellow-soft'
                  : 'rounded-md border border-green/25 bg-green/5 px-3 py-2 text-sm leading-6 text-green-soft'
              }
            >
              {t(
                draft.networkAccess === 'allowed'
                  ? 'settings.network.allowed_explanation'
                  : 'settings.network.blocked_explanation',
              )}
            </div>
          </FormSection>
        )}

        {section === 'profile' && (
          <FormSection title={t('settings.profiles.sections.limits')}>
            <FormRow>
              <FormField
                label={t('settings.profiles.fields.context_tokens')}
                required
              >
                <FormInput
                  type="number"
                  min={1}
                  value={draft.maxContextTokens}
                  onChange={(event) =>
                    setDraft((value) => ({
                      ...value,
                      maxContextTokens: event.target.value,
                    }))
                  }
                />
              </FormField>
              <FormField
                label={t('settings.profiles.fields.duration_minutes')}
                required
              >
                <FormInput
                  type="number"
                  min={1}
                  value={draft.maxInvestigationMinutes}
                  onChange={(event) =>
                    setDraft((value) => ({
                      ...value,
                      maxInvestigationMinutes: event.target.value,
                    }))
                  }
                />
              </FormField>
            </FormRow>
            <FormField
              label={t('settings.profiles.fields.tool_calls')}
              required
            >
              <FormInput
                type="number"
                min={1}
                max={256}
                value={draft.maxToolCalls}
                onChange={(event) =>
                  setDraft((value) => ({
                    ...value,
                    maxToolCalls: event.target.value,
                  }))
                }
              />
            </FormField>
          </FormSection>
        )}

        {section === 'approvals' && (
          <FormSection title={t('settings.profiles.sections.approvals')}>
            {(['l0', 'l1', 'l2', 'l3'] as const).map((risk) => (
              <FormField
                key={risk}
                label={`${risk.toUpperCase()} · ${t(`settings.approval_policy.${risk}`)}`}
              >
                <FormSelect
                  value={draft[`${risk}Policy`]}
                  onChange={(policy) =>
                    setDraft((value) => ({
                      ...value,
                      [`${risk}Policy`]: policy,
                    }))
                  }
                  options={riskOptions}
                />
              </FormField>
            ))}
          </FormSection>
        )}
        {formError && <FormError>{formError}</FormError>}
      </form>
    </FormDrawer>
  );
}

function sectionUpdatedKey(section: ProfileEditorSection): string {
  if (section === 'tools') return 'settings.tools.updated';
  if (section === 'data') return 'settings.data.updated';
  if (section === 'network') return 'settings.network.updated';
  if (section === 'approvals') return 'settings.approval_policy.updated';
  return 'settings.profiles.updated';
}

interface ProviderDraft {
  provider: providersApi.ModelProviderKind;
  name: string;
  baseUrl: string;
  model: string;
  timeoutMs: string;
  maxTokens: string;
  apiKey: string;
  enabled: boolean;
}

function providerDraft(target: Exclude<ProviderEditorTarget, null>): ProviderDraft {
  if (target === 'new') {
    return {
      provider: 'openai',
      name: '',
      baseUrl: '',
      model: '',
      timeoutMs: '30000',
      maxTokens: '',
      apiKey: '',
      enabled: true,
    };
  }
  return {
    provider: target.provider,
    name: target.name,
    baseUrl: target.base_url ?? '',
    model: target.default_model,
    timeoutMs: String(target.timeout_ms),
    maxTokens: target.max_tokens == null ? '' : String(target.max_tokens),
    apiKey: '',
    enabled: target.enabled,
  };
}

export function ModelProviderEditorDrawer({
  target,
  onClose,
}: {
  target: ProviderEditorTarget;
  onClose: () => void;
}) {
  const { t } = useTranslation('intelligence');
  const queryClient = useQueryClient();
  const [draft, setDraft] = React.useState<ProviderDraft>(() => providerDraft('new'));
  const [formError, setFormError] = React.useState('');

  React.useEffect(() => {
    if (!target) return;
    setDraft(providerDraft(target));
    setFormError('');
  }, [target]);

  const save = useMutation({
    mutationFn: async ({
      id,
      input,
      apiKey,
    }: {
      id: string | null;
      input: providersApi.UpdateProviderInput;
      apiKey: string;
    }) => {
      if (!id) {
        return providersApi.create({
          ...input,
          api_key: apiKey || undefined,
        });
      }
      let saved = await providersApi.update(id, input);
      if (apiKey) saved = await providersApi.rotateKey(id, apiKey);
      return saved;
    },
    onSuccess: async (_saved, variables) => {
      await queryClient.invalidateQueries({
        queryKey: ['intelligence', 'model-providers'],
      });
      onClose();
      toast.success(
        t(variables.id ? 'settings.models.updated' : 'settings.models.created'),
      );
    },
    onError: (error) => toast.error(String(error)),
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!target || !draft.name.trim() || !draft.model.trim()) return;
    if (draft.provider === 'openai_compatible' && !draft.baseUrl.trim()) {
      setFormError(t('settings.models.base_url_required'));
      return;
    }
    const timeoutMs = Number(draft.timeoutMs);
    const maxTokens = draft.maxTokens.trim() ? Number(draft.maxTokens) : undefined;
    if (
      !Number.isFinite(timeoutMs) ||
      timeoutMs <= 0 ||
      (maxTokens !== undefined && (!Number.isFinite(maxTokens) || maxTokens <= 0))
    ) {
      setFormError(t('settings.models.invalid_limits'));
      return;
    }
    setFormError('');
    save.mutate({
      id: target === 'new' ? null : target.id,
      input: {
        provider: draft.provider,
        name: draft.name.trim(),
        base_url: draft.baseUrl.trim() || undefined,
        default_model: draft.model.trim(),
        enabled: draft.enabled,
        timeout_ms: Math.round(timeoutMs),
        max_tokens: maxTokens === undefined ? undefined : Math.round(maxTokens),
      },
      apiKey: draft.apiKey.trim(),
    });
  };

  const formId = 'intelligence-model-provider-editor';
  return (
    <FormDrawer
      open={target !== null}
      onOpenChange={(open) => {
        if (!open && !save.isPending) onClose();
      }}
      title={t(target === 'new' ? 'settings.models.create' : 'settings.models.edit')}
      subtitle={t('settings.models.editor_description')}
      width={640}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          onCancel={onClose}
          formId={formId}
          submitLabel={t(target === 'new' ? 'common.create' : 'common.save')}
        />
      }
    >
      <form id={formId} onSubmit={submit}>
        <FormSection title={t('settings.models.sections.connection')}>
          <FormField label={t('settings.models.fields.provider')} required>
            <FormSelect
              value={draft.provider}
              onChange={(provider) =>
                setDraft((value) => ({
                  ...value,
                  provider: provider as providersApi.ModelProviderKind,
                }))
              }
              options={[
                { value: 'openai', label: 'OpenAI' },
                { value: 'anthropic', label: 'Anthropic' },
                { value: 'openai_compatible', label: 'OpenAI-compatible' },
              ]}
            />
          </FormField>
          <FormField label={t('settings.models.fields.name')} required>
            <FormInput
              value={draft.name}
              onChange={(event) => setDraft((value) => ({ ...value, name: event.target.value }))}
              placeholder={t('settings.models.name_placeholder')}
              autoFocus
            />
          </FormField>
          <FormField
            label={t('settings.models.fields.base_url')}
            required={draft.provider === 'openai_compatible'}
            hint={t('settings.models.base_url_hint')}
          >
            <FormInput
              type="url"
              value={draft.baseUrl}
              onChange={(event) =>
                setDraft((value) => ({ ...value, baseUrl: event.target.value }))
              }
              placeholder={t('settings.models.base_url_placeholder')}
            />
          </FormField>
          <FormField label={t('settings.models.fields.model')} required>
            <FormInput
              value={draft.model}
              onChange={(event) =>
                setDraft((value) => ({ ...value, model: event.target.value }))
              }
              placeholder={t('settings.models.model_placeholder')}
            />
          </FormField>
        </FormSection>

        <FormSection title={t('settings.models.sections.runtime')}>
          <FormRow>
            <FormField label={t('settings.models.fields.timeout_ms')} required>
              <FormInput
                type="number"
                min={1}
                value={draft.timeoutMs}
                onChange={(event) =>
                  setDraft((value) => ({ ...value, timeoutMs: event.target.value }))
                }
              />
            </FormField>
            <FormField label={t('settings.models.fields.max_tokens')}>
              <FormInput
                type="number"
                min={1}
                value={draft.maxTokens}
                onChange={(event) =>
                  setDraft((value) => ({ ...value, maxTokens: event.target.value }))
                }
                placeholder={t('settings.models.max_tokens_placeholder')}
              />
            </FormField>
          </FormRow>
          <SwitchRow
            label={t('settings.models.fields.enabled')}
            checked={draft.enabled}
            onCheckedChange={(enabled) => setDraft((value) => ({ ...value, enabled }))}
          />
        </FormSection>

        <FormSection
          title={t('settings.models.sections.credentials')}
          description={t(
            target === 'new'
              ? 'settings.models.key_create_hint'
              : 'settings.models.key_edit_hint',
          )}
        >
          <FormField label={t('settings.models.fields.api_key')}>
            <FormInput
              type="password"
              autoComplete="new-password"
              value={draft.apiKey}
              onChange={(event) =>
                setDraft((value) => ({ ...value, apiKey: event.target.value }))
              }
              placeholder={t('settings.models.key_placeholder')}
            />
          </FormField>
        </FormSection>
        {formError && <FormError>{formError}</FormError>}
      </form>
    </FormDrawer>
  );
}

function SwitchRow({
  label,
  checked,
  onCheckedChange,
}: {
  label: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex min-h-11 items-center gap-3 rounded-md border border-bd-0 bg-bg-2 px-3">
      <span className="min-w-0 flex-1 text-sm font-strong text-tx-1">{label}</span>
      <Switch
        checked={checked}
        onCheckedChange={onCheckedChange}
        aria-label={label}
      />
    </div>
  );
}

function FormError({ children }: { children: React.ReactNode }) {
  return (
    <div role="alert" className="rounded-md border border-red/35 bg-red/5 px-3 py-2 text-sm text-red-soft">
      {children}
    </div>
  );
}
