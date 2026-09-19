import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as providersApi from '@/api/agent/modelProviders';
import { toApiError } from '@/lib/http';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

export type ProviderEditorTarget = providersApi.ModelProvider | 'new' | null;

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
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const [draft, setDraft] = React.useState<ProviderDraft>(() =>
    providerDraft('new'),
  );
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
        queryKey: ['agent', 'model-providers'],
      });
      onClose();
      toast.success(
        t(variables.id ? 'settings.models.updated' : 'settings.models.created'),
      );
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!target || !draft.name.trim() || !draft.model.trim()) return;
    if (draft.provider === 'openai_compatible' && !draft.baseUrl.trim()) {
      setFormError(t('settings.models.base_url_required'));
      return;
    }
    const timeoutMs = Number(draft.timeoutMs);
    const maxTokens = draft.maxTokens.trim()
      ? Number(draft.maxTokens)
      : undefined;
    if (
      !Number.isFinite(timeoutMs) ||
      timeoutMs <= 0 ||
      (maxTokens !== undefined &&
        (!Number.isFinite(maxTokens) || maxTokens <= 0))
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

  const formId = 'agent-model-provider-editor';
  return (
    <FormDrawer
      open={target !== null}
      onOpenChange={(open) => {
        if (!open && !save.isPending) onClose();
      }}
      title={t(
        target === 'new' ? 'settings.models.create' : 'settings.models.edit',
      )}
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
              onChange={(event) =>
                setDraft((value) => ({ ...value, name: event.target.value }))
              }
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
                setDraft((value) => ({
                  ...value,
                  baseUrl: event.target.value,
                }))
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
                  setDraft((value) => ({
                    ...value,
                    timeoutMs: event.target.value,
                  }))
                }
              />
            </FormField>
            <FormField label={t('settings.models.fields.max_tokens')}>
              <FormInput
                type="number"
                min={1}
                value={draft.maxTokens}
                onChange={(event) =>
                  setDraft((value) => ({
                    ...value,
                    maxTokens: event.target.value,
                  }))
                }
                placeholder={t('settings.models.max_tokens_placeholder')}
              />
            </FormField>
          </FormRow>
          <SwitchRow
            label={t('settings.models.fields.enabled')}
            checked={draft.enabled}
            onCheckedChange={(enabled) =>
              setDraft((value) => ({ ...value, enabled }))
            }
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
    <div
      role="alert"
      className="rounded-md border border-red/35 bg-red/5 px-3 py-2 text-sm text-red-soft"
    >
      {children}
    </div>
  );
}
