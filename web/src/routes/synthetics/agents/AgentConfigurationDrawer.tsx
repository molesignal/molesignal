import { useMutation } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { LockKeyhole, Plus, SlidersHorizontal, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as syntheticsApi from '@/api/synthetics';
import type { ProbeAgent, UpdateAgentConfigurationInput } from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import { ChromeButton, IconButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import {
  isReservedAgentLabel,
  validateAgentConfiguration,
  type AgentConfigurationError,
} from './model';

interface LabelDraft {
  id: number;
  key: string;
  value: string;
}

let nextLabelId = 0;

function labelDraft(key = '', value = ''): LabelDraft {
  nextLabelId += 1;
  return { id: nextLabelId, key, value };
}

export function AgentConfigurationDrawer({
  agent,
  onClose,
  onUpdated,
}: {
  agent: ProbeAgent | undefined;
  onClose: () => void;
  onUpdated: (agent: ProbeAgent) => void | Promise<void>;
}) {
  const { t } = useTranslation('synthetics');
  const [name, setName] = React.useState('');
  const [labels, setLabels] = React.useState<LabelDraft[]>([]);

  React.useEffect(() => {
    if (!agent) return;
    setName(agent.name);
    setLabels(
      Object.entries(agent.labels)
        .filter(([key]) => !isReservedAgentLabel(key))
        .map(([key, value]) => labelDraft(key, value)),
    );
  }, [agent]);

  const managedLabels = React.useMemo(
    () =>
      Object.entries(agent?.labels ?? {}).filter(([key]) => isReservedAgentLabel(key)),
    [agent?.labels],
  );
  const validationError = validateAgentConfiguration(
    name,
    labels,
    managedLabels.length,
  );
  const save = useMutation({
    mutationFn: (input: UpdateAgentConfigurationInput) => {
      if (!agent) throw new Error('Agent configuration target is missing');
      return syntheticsApi.updateAgentConfiguration(agent.id, input);
    },
    onSuccess: async (updated) => {
      toast.success(t('agents.configuration_updated'));
      await onUpdated(updated);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const setLabel = (id: number, patch: Partial<Pick<LabelDraft, 'key' | 'value'>>) => {
    setLabels((current) =>
      current.map((label) => (label.id === id ? { ...label, ...patch } : label)),
    );
  };
  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!agent || validationError) return;
    save.mutate({
      name: name.trim(),
      labels: Object.fromEntries(
        labels.map((label) => [label.key.trim(), label.value.trim()]),
      ),
    });
  };
  const validationMessage = configurationErrorMessage(t, validationError);

  return (
    <FormDrawer
      open={Boolean(agent)}
      onOpenChange={(open) => !open && onClose()}
      title={t('agents.edit_configuration_title')}
      subtitle={agent ? t('agents.edit_configuration_subtitle', { hostname: agent.hostname }) : undefined}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          invalid={Boolean(validationError)}
          disabledReason={validationMessage}
          onCancel={onClose}
          formId="synthetic-agent-configuration-form"
          submitLabel={t('actions.save_configuration')}
        />
      }
    >
      <form id="synthetic-agent-configuration-form" onSubmit={submit}>
        <div className="mb-6 flex gap-3 rounded-md border border-bd-0 bg-bg-2 p-3 text-xs leading-relaxed text-tx-2">
          <SlidersHorizontal aria-hidden className="mt-0.5 h-4 w-4 shrink-0 text-indigo-soft" />
          {t('agents.configuration_scope_hint')}
        </div>

        <FormSection
          title={t('agents.display_metadata')}
          description={t('agents.display_metadata_hint')}
        >
          <FormField label={t('agents.display_name')} required>
            <FormInput
              autoFocus
              value={name}
              maxLength={255}
              onChange={(event) => setName(event.target.value)}
              className="h-11 text-base sm:h-9 sm:text-sm"
            />
          </FormField>
        </FormSection>

        <FormSection title={t('agents.custom_labels')} description={t('agents.custom_labels_hint')}>
          {labels.length === 0 ? (
            <div className="rounded-md border border-dashed border-bd-1 px-4 py-5 text-center text-xs text-tx-3">
              {t('agents.no_custom_labels')}
            </div>
          ) : (
            <div className="space-y-3">
              {labels.map((label) => (
                <div
                  key={label.id}
                  className="grid grid-cols-1 gap-2 rounded-md border border-bd-0 bg-bg-2 p-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_32px] sm:items-end"
                >
                  <FormField label={t('agents.label_key')}>
                    <FormInput
                      aria-label={t('agents.label_key')}
                      value={label.key}
                      maxLength={64}
                      onChange={(event) => setLabel(label.id, { key: event.target.value })}
                      placeholder="environment"
                      className="h-11 text-base sm:h-9 sm:text-sm"
                    />
                  </FormField>
                  <FormField label={t('agents.label_value')}>
                    <FormInput
                      aria-label={t('agents.label_value')}
                      value={label.value}
                      maxLength={255}
                      onChange={(event) => setLabel(label.id, { value: event.target.value })}
                      placeholder="production"
                      className="h-11 text-base sm:h-9 sm:text-sm"
                    />
                  </FormField>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <IconButton
                        aria-label={t('agents.remove_label')}
                        onClick={() =>
                          setLabels((current) => current.filter((item) => item.id !== label.id))
                        }
                        className="h-11 w-11 justify-self-end text-tx-3 hover:text-red-soft sm:h-8 sm:w-8"
                      >
                        <Trash2 aria-hidden className="h-3.5 w-3.5" />
                      </IconButton>
                    </TooltipTrigger>
                    <TooltipContent>{t('agents.remove_label')}</TooltipContent>
                  </Tooltip>
                </div>
              ))}
            </div>
          )}
          <div>
            <ChromeButton
              type="button"
              disabled={labels.length + managedLabels.length >= 32}
              disabledReason={t('agents.configuration_validation.too_many_labels')}
              onClick={() => setLabels((current) => [...current, labelDraft()])}
            >
              <Plus aria-hidden className="h-3.5 w-3.5" />
              {t('agents.add_label')}
            </ChromeButton>
          </div>
          {validationMessage && (
            <p role="alert" className="text-xs text-red-soft">
              {validationMessage}
            </p>
          )}
        </FormSection>

        {managedLabels.length > 0 && (
          <FormSection
            title={t('agents.managed_labels')}
            description={t('agents.managed_labels_hint')}
          >
            <div className="divide-y divide-bd-0 overflow-hidden rounded-md border border-bd-0 bg-bg-2">
              {managedLabels.map(([key, value]) => (
                <div
                  key={key}
                  className="grid grid-cols-[minmax(120px,0.5fr)_minmax(0,1fr)_16px] items-center gap-3 px-3 py-2.5 text-xs"
                >
                  <span className="font-code text-tx-3">{key}</span>
                  <span className="min-w-0 break-all font-code text-tx-1">{value}</span>
                  <LockKeyhole aria-hidden className="h-3.5 w-3.5 text-tx-3" />
                </div>
              ))}
            </div>
          </FormSection>
        )}
      </form>
    </FormDrawer>
  );
}

function configurationErrorMessage(
  t: TFunction<'synthetics'>,
  error: AgentConfigurationError | undefined,
): string | undefined {
  return error ? t(`agents.configuration_validation.${error}`) : undefined;
}
