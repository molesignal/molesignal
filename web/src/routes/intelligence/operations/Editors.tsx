import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as intelligenceApi from '@/api/intelligence';
import { useActionAccess } from '@/product/actionAccess';
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

import {
  formatJsonEditor,
  parseDelimitedList,
  parseJsonEditor,
  stringListValue,
} from '../editorModel';
import { IntelligenceToolChecklist } from '../IntelligenceToolChecklist';

export type InvestigationEditorTarget =
  | intelligenceApi.Investigation
  | 'new'
  | null;
export type AutomationEditorTarget = intelligenceApi.Automation | 'new' | null;

interface InvestigationDraft {
  title: string;
  status: intelligenceApi.InvestigationStatus;
  summary: string;
  confidence: intelligenceApi.ConfidenceLevel | '';
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
    currentStep: target.current_step ?? '',
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
  const { t } = useTranslation('intelligence');
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
        return intelligenceApi.createInvestigation({
          title: draft.title.trim(),
          context,
          steps: parseDelimitedList(draft.steps),
        });
      }
      return intelligenceApi.updateInvestigation(id, {
        title: draft.title.trim(),
        status: draft.status,
        summary: draft.summary.trim() || null,
        confidence: draft.confidence || null,
        current_step: draft.currentStep.trim() || null,
        context,
      });
    },
    onSuccess: async (saved, variables) => {
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: ['intelligence', 'investigations'],
        }),
        queryClient.invalidateQueries({
          queryKey: ['intelligence', 'investigation', saved.id],
        }),
        queryClient.invalidateQueries({ queryKey: ['intelligence', 'overview'] }),
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
  const formId = 'intelligence-investigation-editor';
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
                        status: status as intelligenceApi.InvestigationStatus,
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
                          confidence as intelligenceApi.ConfidenceLevel | '',
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

interface AutomationDraft {
  name: string;
  description: string;
  enabled: boolean;
  trigger: string;
  inputContext: string;
  steps: string;
  allowedTools: string[];
  approvalPolicy: string;
  outputActions: string;
  failurePolicy: string;
  notification: string;
}

function automationDraft(
  target: Exclude<AutomationEditorTarget, null>,
  defaultTools: string[],
  defaultSteps: string[],
  defaultDescription: string,
): AutomationDraft {
  if (target === 'new') {
    return {
      name: '',
      description: defaultDescription,
      enabled: true,
      trigger: formatJsonEditor({ type: 'manual' }),
      inputContext: formatJsonEditor({}),
      steps: formatJsonEditor(defaultSteps),
      allowedTools: defaultTools,
      approvalPolicy: formatJsonEditor({ write_operations: 'required' }),
      outputActions: formatJsonEditor([]),
      failurePolicy: formatJsonEditor({ strategy: 'stop' }),
      notification: formatJsonEditor({}),
    };
  }
  return {
    name: target.name,
    description: target.description,
    enabled: target.enabled,
    trigger: formatJsonEditor(target.trigger),
    inputContext: formatJsonEditor(target.input_context),
    steps: formatJsonEditor(target.steps, []),
    allowedTools: target.allowed_tools,
    approvalPolicy: formatJsonEditor(target.approval_policy),
    outputActions: formatJsonEditor(target.output_actions, []),
    failurePolicy: formatJsonEditor(target.failure_policy),
    notification: formatJsonEditor(target.notification),
  };
}

type ParsedAutomation = Omit<
  intelligenceApi.AutomationInput,
  'name' | 'description' | 'enabled' | 'allowed_tools'
>;

export function AutomationEditorDrawer({
  target,
  tools,
  onClose,
}: {
  target: AutomationEditorTarget;
  tools: intelligenceApi.RegisteredTool[];
  onClose: () => void;
}) {
  const { t } = useTranslation('intelligence');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'intelligence.manage',
  });
  const defaultTools = React.useMemo(
    () =>
      tools.length
        ? tools.map((tool) => tool.name)
        : [
            'query_logs',
            'query_metrics',
            'list_streams',
            'get_trace',
            'list_recent_alerts',
            'list_on_call_schedules',
            'get_current_on_call',
            'list_rum_sessions',
            'list_rum_actions',
            'list_rum_errors',
            'list_continuous_profiles',
            'list_report_templates',
            'list_scheduled_reports',
          ],
    [tools],
  );
  const defaultSteps = React.useMemo(
    () => [
      t('automations.default_steps.metrics'),
      t('automations.default_steps.logs'),
      t('automations.default_steps.traces'),
      t('automations.default_steps.hypothesis'),
    ],
    [t],
  );
  const [draft, setDraft] = React.useState<AutomationDraft>(() =>
    automationDraft(
      'new',
      defaultTools,
      defaultSteps,
      t('automations.default_description'),
    ),
  );
  const [formError, setFormError] = React.useState('');

  React.useEffect(() => {
    if (!target) return;
    setDraft(
      automationDraft(
        target,
        defaultTools,
        defaultSteps,
        t('automations.default_description'),
      ),
    );
    setFormError('');
  }, [defaultSteps, defaultTools, t, target]);

  const save = useMutation({
    mutationFn: ({
      id,
      input,
    }: {
      id: string | null;
      input: intelligenceApi.AutomationInput;
    }) =>
      id
        ? intelligenceApi.updateAutomation(id, input)
        : intelligenceApi.createAutomation(input),
    onSuccess: async (_saved, variables) => {
      await queryClient.invalidateQueries({
        queryKey: ['intelligence', 'automations'],
      });
      onClose();
      toast.success(
        t(variables.id ? 'automations.updated' : 'automations.created'),
      );
    },
    onError: (error) => toast.error(String(error)),
  });

  const parseAutomation = (): ParsedAutomation | null => {
    const fields = [
      {
        key: 'trigger',
        label: t('automations.fields.trigger'),
        source: draft.trigger,
        object: true,
      },
      {
        key: 'input_context',
        label: t('automations.fields.input_context'),
        source: draft.inputContext,
        object: true,
      },
      {
        key: 'steps',
        label: t('automations.fields.steps'),
        source: draft.steps,
        object: false,
      },
      {
        key: 'approval_policy',
        label: t('automations.fields.approval_policy'),
        source: draft.approvalPolicy,
        object: true,
      },
      {
        key: 'output_actions',
        label: t('automations.fields.output_actions'),
        source: draft.outputActions,
        object: false,
      },
      {
        key: 'failure_policy',
        label: t('automations.fields.failure_policy'),
        source: draft.failurePolicy,
        object: true,
      },
      {
        key: 'notification',
        label: t('automations.fields.notification'),
        source: draft.notification,
        object: true,
      },
    ] as const;
    const parsed: Partial<ParsedAutomation> = {};
    for (const field of fields) {
      const result = parseJsonEditor(field.source);
      if (!result.ok) {
        setFormError(
          t('common.invalid_json', {
            field: field.label,
            message: result.message,
          }),
        );
        return null;
      }
      if (field.object && !isJsonObject(result.value)) {
        setFormError(t('common.json_object_required', { field: field.label }));
        return null;
      }
      Object.assign(parsed, { [field.key]: result.value });
    }
    return parsed as ParsedAutomation;
  };

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!manageAccess.allowed || !target || !draft.name.trim()) return;
    const parsed = parseAutomation();
    if (!parsed) return;
    setFormError('');
    save.mutate({
      id: target === 'new' ? null : target.id,
      input: {
        name: draft.name.trim(),
        description: draft.description.trim(),
        enabled: draft.enabled,
        allowed_tools: draft.allowedTools,
        ...parsed,
      },
    });
  };

  const isNew = target === 'new';
  const formId = 'intelligence-automation-editor';
  return (
    <FormDrawer
      open={target !== null}
      onOpenChange={(open) => {
        if (!open && !save.isPending) onClose();
      }}
      title={t(isNew ? 'automations.create' : 'automations.edit')}
      subtitle={t('automations.editor_description')}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={manageAccess.disabled}
          invalid={!draft.name.trim()}
          disabledReason={manageAccess.reason}
          onCancel={onClose}
          formId={formId}
          submitLabel={t(isNew ? 'common.create' : 'common.save')}
        />
      }
    >
      <form id={formId} onSubmit={submit}>
        {manageAccess.disabled && manageAccess.reason && (
          <div
            role="status"
            className="mb-4 rounded-md border border-bd-1 bg-bg-2 px-3 py-2 font-sans text-xs text-tx-2"
          >
            {manageAccess.reason}
          </div>
        )}
        <fieldset
          disabled={manageAccess.disabled || save.isPending}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
        >
        <FormSection title={t('automations.sections.basic')}>
          <FormField label={t('automations.fields.name')} required>
            <FormInput
              value={draft.name}
              onChange={(event) =>
                setDraft((value) => ({ ...value, name: event.target.value }))
              }
              placeholder={t('automations.name_placeholder')}
              autoFocus
            />
          </FormField>
          <FormField label={t('automations.fields.description')}>
            <FormTextarea
              value={draft.description}
              onChange={(event) =>
                setDraft((value) => ({
                  ...value,
                  description: event.target.value,
                }))
              }
            />
          </FormField>
          <SwitchRow
            label={t('automations.fields.enabled')}
            checked={draft.enabled}
            onCheckedChange={(enabled) =>
              setDraft((value) => ({ ...value, enabled }))
            }
          />
        </FormSection>

        <FormSection
          title={t('automations.sections.tools')}
          description={t('automations.sections.tools_description')}
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
          {!tools.length && (
            <FormField
              label={t('automations.fields.allowed_tools')}
              hint={t('automations.tools_fallback_hint')}
            >
              <FormTextarea
                value={stringListValue(draft.allowedTools)}
                onChange={(event) =>
                  setDraft((value) => ({
                    ...value,
                    allowedTools: parseDelimitedList(event.target.value),
                  }))
                }
              />
            </FormField>
          )}
        </FormSection>

        <FormSection
          title={t('automations.sections.workflow')}
          description={t('automations.json_hint')}
        >
          <JsonField
            label={t('automations.fields.trigger')}
            value={draft.trigger}
            onChange={(trigger) => setDraft((value) => ({ ...value, trigger }))}
          />
          <JsonField
            label={t('automations.fields.input_context')}
            value={draft.inputContext}
            onChange={(inputContext) =>
              setDraft((value) => ({ ...value, inputContext }))
            }
          />
          <JsonField
            label={t('automations.fields.steps')}
            value={draft.steps}
            onChange={(steps) => setDraft((value) => ({ ...value, steps }))}
            tall
          />
        </FormSection>

        <FormSection title={t('automations.sections.governance')}>
          <JsonField
            label={t('automations.fields.approval_policy')}
            value={draft.approvalPolicy}
            onChange={(approvalPolicy) =>
              setDraft((value) => ({ ...value, approvalPolicy }))
            }
          />
          <JsonField
            label={t('automations.fields.output_actions')}
            value={draft.outputActions}
            onChange={(outputActions) =>
              setDraft((value) => ({ ...value, outputActions }))
            }
          />
          <JsonField
            label={t('automations.fields.failure_policy')}
            value={draft.failurePolicy}
            onChange={(failurePolicy) =>
              setDraft((value) => ({ ...value, failurePolicy }))
            }
          />
          <JsonField
            label={t('automations.fields.notification')}
            value={draft.notification}
            onChange={(notification) =>
              setDraft((value) => ({ ...value, notification }))
            }
          />
        </FormSection>
        {formError && <FormError>{formError}</FormError>}
        </fieldset>
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
      <FormTextarea
        value={value}
        onChange={(event) => onChange(event.target.value)}
        spellCheck={false}
        className={tall ? 'min-h-48 font-mono text-xs' : 'min-h-32 font-mono text-xs'}
      />
    </FormField>
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

function isJsonObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
