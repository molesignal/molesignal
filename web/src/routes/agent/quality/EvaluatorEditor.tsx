import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ModelProvider } from '@/api/agent/modelProviders';
import * as qualityApi from '@/api/agent/quality';
import type { Evaluator } from '@/api/agent/quality';
import { toApiError } from '@/lib/http';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormGroup,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import {
  draftFromEvaluator,
  inputFromDraft,
  EVALUATOR_TARGETS,
  type EvaluatorDraft,
  evaluatorProblems,
  newEvaluatorDraft,
} from './model';
import { QUALITY_KEY } from './state';

const FORM_ID = 'agent-evaluator-editor';
const NAME_MAX = 80;
const RUBRIC_MAX = 4000;

/**
 * An evaluator: the rubric a model scores answers by, which answers it scores and
 * how many of the new ones, and the model it uses. Saving it makes the
 * saver's interface language the one it writes its reasons in.
 */
export function EvaluatorEditor({
  evaluator,
  providers,
  onClose,
}: {
  /** The evaluator being changed; null for a new one. */
  evaluator: Evaluator | null;
  providers: readonly ModelProvider[];
  onClose: () => void;
}) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const [draft, setDraft] = React.useState<EvaluatorDraft>(() =>
    evaluator ? draftFromEvaluator(evaluator) : newEvaluatorDraft(t),
  );
  const change = (patch: Partial<EvaluatorDraft>) => setDraft((current) => ({ ...current, ...patch }));
  const problems = evaluatorProblems(draft);

  const save = useMutation({
    mutationFn: () =>
      evaluator
        ? qualityApi.updateEvaluator(evaluator.id, inputFromDraft(draft))
        : qualityApi.createEvaluator(inputFromDraft(draft)),
    onSuccess: (saved) => {
      toast.success(t(evaluator ? 'quality.evaluators.saved' : 'quality.evaluators.created', { name: saved.name }));
      void queryClient.invalidateQueries({ queryKey: QUALITY_KEY });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const providerOptions = [
    { value: '', label: t('quality.evaluators.provider_default') },
    ...providers
      .filter((provider) => provider.enabled)
      .map((provider) => ({ value: provider.id, label: provider.name })),
  ];
  const chosen = providers.find((provider) => provider.id === draft.providerId);

  return (
    <FormDrawer
      open
      onOpenChange={(open) => !open && onClose()}
      title={t(evaluator ? 'quality.evaluators.edit_title' : 'quality.evaluators.new_title')}
      subtitle={t('quality.evaluators.subtitle')}
      width={640}
      footer={
        <FormSubmitFooter
          formId={FORM_ID}
          busy={save.isPending}
          invalid={problems.length > 0}
          onCancel={onClose}
          submitLabel={t(evaluator ? 'quality.evaluators.save' : 'quality.evaluators.create')}
        />
      }
    >
      <form
        id={FORM_ID}
        data-evaluator-editor={evaluator ? 'edit' : 'new'}
        onSubmit={(event) => {
          event.preventDefault();
          event.stopPropagation();
          if (problems.length === 0 && !save.isPending) save.mutate();
        }}
      >
        <FormSection title={t('quality.evaluators.what')}>
          <FormField label={t('quality.evaluators.name')} required>
            <FormInput
              value={draft.name}
              maxLength={NAME_MAX}
              onChange={(event) => change({ name: event.target.value })}
            />
          </FormField>
          <FormField label={t('quality.evaluators.rubric')} hint={t('quality.evaluators.rubric_hint')} required>
            <FormTextarea
              value={draft.rubric}
              rows={8}
              maxLength={RUBRIC_MAX}
              onChange={(event) => change({ rubric: event.target.value })}
            />
          </FormField>
        </FormSection>
        <FormSection title={t('quality.evaluators.which')}>
          <FormGroup label={t('quality.evaluators.targets')} hint={t('quality.evaluators.targets_hint')} required>
            <FormChecklist
              options={EVALUATOR_TARGETS.map((target) => ({
                value: target,
                label: t(`quality.sources.${target}`),
              }))}
              selected={draft.targets}
              onChange={(targets) => change({ targets })}
            />
          </FormGroup>
          <FormField label={t('quality.evaluators.sample')} hint={t('quality.evaluators.sample_hint')}>
            <FormInput
              type="number"
              min={0}
              max={100}
              step={5}
              value={String(draft.samplePercent)}
              onChange={(event) => change({ samplePercent: Number(event.target.value) || 0 })}
            />
          </FormField>
        </FormSection>
        <FormSection title={t('quality.evaluators.model_section')}>
          <FormRow>
            <FormField label={t('quality.evaluators.provider')}>
              <FormSelect
                ariaLabel={t('quality.evaluators.provider')}
                value={draft.providerId}
                onChange={(providerId) => change({ providerId })}
                options={providerOptions}
              />
            </FormField>
            <FormField label={t('quality.evaluators.model')}>
              <FormInput
                value={draft.model}
                placeholder={chosen?.defaultModel ?? t('quality.evaluators.model_default')}
                onChange={(event) => change({ model: event.target.value })}
              />
            </FormField>
          </FormRow>
          <FormField label={t('quality.evaluators.temperature')} hint={t('quality.evaluators.temperature_hint')}>
            <FormInput
              type="number"
              min={0}
              max={1}
              step={0.1}
              value={String(draft.temperature)}
              onChange={(event) => change({ temperature: Number(event.target.value) || 0 })}
            />
          </FormField>
          <label className="flex items-center gap-2 font-sans text-xs text-tx-1">
            <Switch checked={draft.enabled} onCheckedChange={(enabled) => change({ enabled })} />
            {t('quality.evaluators.enabled')}
          </label>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
