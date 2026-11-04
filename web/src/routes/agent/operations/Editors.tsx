import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as agentApi from '@/api/agent';
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
import { JsonEditor } from '@/shell/JsonEditor';
import { toast } from '@/shell/ui/sonner';

import {
  formatJsonEditor,
  parseDelimitedList,
  parseJsonEditor,
} from '../editorModel';

export type InvestigationEditorTarget =
  | agentApi.Investigation
  | 'new'
  | null;

interface InvestigationDraft {
  title: string;
  status: agentApi.InvestigationStatus;
  summary: string;
  confidence: agentApi.ConfidenceLevel | '';
  currentStep: string;
  context: string;
  steps: string;
}

function investigationDraft(
  target: Exclude<InvestigationEditorTarget, null>,
  defaultSteps: string[],
): InvestigationDraft {
  if (target === 'new') {
    return {
      title: '',
      status: 'draft',
      summary: '',
      confidence: '',
      currentStep: '',
      context: formatJsonEditor({}),
      steps: defaultSteps.join('\n'),
    };
  }
  return {
    title: target.title,
    status: target.status,
    summary: target.summary ?? '',
    confidence: target.confidence ?? '',
    currentStep: target.currentStep ?? '',
    context: formatJsonEditor(target.context),
    steps: '',
  };
}

export function InvestigationEditorDrawer({
  target,
  onClose,
}: {
  target: InvestigationEditorTarget;
  onClose: () => void;
}) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const defaultSteps = React.useMemo(
    () => [
      t('investigations.default_steps.context'),
      t('investigations.default_steps.metrics'),
      t('investigations.default_steps.logs'),
      t('investigations.default_steps.traces'),
      t('investigations.default_steps.hypothesis'),
      t('investigations.default_steps.recommendation'),
    ],
    [t],
  );
  const [draft, setDraft] = React.useState<InvestigationDraft>(() =>
    investigationDraft('new', defaultSteps),
  );
  const [formError, setFormError] = React.useState('');

  React.useEffect(() => {
    if (!target) return;
    setDraft(investigationDraft(target, defaultSteps));
    setFormError('');
  }, [defaultSteps, target]);

  const save = useMutation({
    mutationFn: async ({
      id,
      context,
    }: {
      id: string | null;
      context: Record<string, unknown>;
    }) => {
      if (!id) {
        return agentApi.createInvestigation({
          title: draft.title.trim(),
          context,
          steps: parseDelimitedList(draft.steps),
        });
      }
      return agentApi.updateInvestigation(id, {
        title: draft.title.trim(),
        status: draft.status,
        summary: draft.summary.trim() || null,
        confidence: draft.confidence || null,
        currentStep: draft.currentStep.trim() || null,
        context,
      });
    },
    onSuccess: async (saved, variables) => {
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: ['agent', 'investigations'],
        }),
        queryClient.invalidateQueries({
          queryKey: ['agent', 'investigation', saved.id],
        }),
        queryClient.invalidateQueries({ queryKey: ['agent', 'overview'] }),
      ]);
      onClose();
      toast.success(
        t(variables.id ? 'investigations.updated' : 'investigations.created'),
      );
    },
    onError: (error) => toast.error(String(error)),
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!target || !draft.title.trim()) return;
    const context = parseJsonEditor(draft.context);
    if (!context.ok) {
      setFormError(
        t('common.invalid_json', {
          field: t('investigations.fields.context'),
          message: context.message,
        }),
      );
      return;
    }
    if (!isJsonObject(context.value)) {
      setFormError(
        t('common.json_object_required', {
          field: t('investigations.fields.context'),
        }),
      );
      return;
    }
    setFormError('');
    save.mutate({
      id: target === 'new' ? null : target.id,
      context: context.value,
    });
  };

  const isNew = target === 'new';
  const formId = 'agent-investigation-editor';
  return (
    <FormDrawer
      open={target !== null}
      onOpenChange={(open) => {
        if (!open && !save.isPending) onClose();
      }}
      title={t(isNew ? 'investigations.create' : 'investigations.edit')}
      subtitle={t('investigations.editor_description')}
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
        <FormSection title={t('investigations.sections.basic')}>
          <FormField label={t('investigations.fields.title')} required>
            <FormInput
              value={draft.title}
              onChange={(event) =>
                setDraft((value) => ({ ...value, title: event.target.value }))
              }
              placeholder={t('investigations.title_placeholder')}
              autoFocus
            />
          </FormField>
          {!isNew && (
            <>
              <FormRow>
                <FormField label={t('common.status')}>
                  <FormSelect
                    value={draft.status}
                    onChange={(status) =>
                      setDraft((value) => ({
                        ...value,
                        status: status as agentApi.InvestigationStatus,
                      }))
                    }
                    options={[
                      'draft',
                      'pending',
                      'running',
                      'waiting_for_data',
                      'waiting_for_approval',
                      'verifying_recovery',
                      'completed',
                      'partially_completed',
                      'failed',
                      'cancelled',
                    ].map((status) => ({
                      value: status,
                      label: t(`status.${status}`),
                    }))}
                  />
                </FormField>
                <FormField label={t('investigations.fields.confidence')}>
                  <FormSelect
                    value={draft.confidence}
                    onChange={(confidence) =>
                      setDraft((value) => ({
                        ...value,
                        confidence:
                          confidence as agentApi.ConfidenceLevel | '',
                      }))
                    }
                    options={[
                      { value: '', label: t('investigations.confidence_unset') },
                      ...(['low', 'medium', 'high'] as const).map((confidence) => ({
                        value: confidence,
                        label: t(`confidence.${confidence}`),
                      })),
                    ]}
                  />
                </FormField>
              </FormRow>
              <FormField label={t('investigations.fields.current_step')}>
                <FormInput
                  value={draft.currentStep}
                  onChange={(event) =>
                    setDraft((value) => ({
                      ...value,
                      currentStep: event.target.value,
                    }))
                  }
                />
              </FormField>
              <FormField label={t('investigations.fields.summary')}>
                <FormTextarea
                  value={draft.summary}
                  onChange={(event) =>
                    setDraft((value) => ({
                      ...value,
                      summary: event.target.value,
                    }))
                  }
                />
              </FormField>
            </>
          )}
        </FormSection>

        {isNew && (
          <FormSection title={t('investigations.sections.steps')}>
            <FormField
              label={t('investigations.fields.steps')}
              hint={t('investigations.steps_hint')}
            >
              <FormTextarea
                value={draft.steps}
                onChange={(event) =>
                  setDraft((value) => ({ ...value, steps: event.target.value }))
                }
                className="min-h-40"
              />
            </FormField>
          </FormSection>
        )}

        <FormSection title={t('investigations.sections.context')}>
          <JsonField
            label={t('investigations.fields.context')}
            value={draft.context}
            onChange={(context) => setDraft((value) => ({ ...value, context }))}
          />
        </FormSection>
        {formError && <FormError>{formError}</FormError>}
      </form>
    </FormDrawer>
  );
}

function JsonField({
  label,
  value,
  onChange,
  tall = false,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  tall?: boolean;
}) {
  return (
    <FormField label={label}>
      <JsonEditor
        value={value}
        onChange={onChange}
        ariaLabel={label}
        minHeight={tall ? 192 : 128}
      />
    </FormField>
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

function isJsonObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
