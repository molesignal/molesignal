import { AlertTriangle } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { StatusPageComponent } from '@/api/statusPages';
import type {
  ActiveAutomationRule,
  AlertSeverity,
  AutomationPublicationMode,
  AutomationRuleInput,
  AutomationSourceKind,
  IncidentImpact,
} from '@/api/statusPages';
import { ChromeButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';

import { findAutomationRuleOverlaps } from './ruleOverlap';
import { SourceResourceField } from './SourceResourceField';

interface RuleEditorProps {
  open: boolean;
  rule: ActiveAutomationRule | null;
  rules: ActiveAutomationRule[];
  components: StatusPageComponent[];
  saving: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (input: AutomationRuleInput) => void;
}

export function RuleEditor({
  open,
  rule,
  rules,
  components,
  saving,
  onOpenChange,
  onSubmit,
}: RuleEditorProps) {
  const { t } = useTranslation('status-pages');
  const [form, setForm] = React.useState(() => editorState(rule));
  React.useEffect(() => setForm(editorState(rule)), [rule, open]);
  // A rule publishes on components; a group shows what they show.
  const activeComponents = components.filter(
    (component) => component.kind !== 'group' && component.lifecycle === 'active',
  );
  const delayMinutes = Number(form.delayMinutes);
  // A dependency rule that names no components publishes on the component whose dependencies changed.
  const componentsOptional = form.sourceKind === 'component_dependencies';
  const invalid =
    !form.name.trim()
    || (form.componentIds.length === 0 && !componentsOptional)
    || !form.title.trim()
    || !form.investigating.trim()
    || !form.update.trim()
    || !form.resolved.trim()
    || !form.correlation.trim()
    || !Number.isFinite(delayMinutes)
    || delayMinutes < 0
    || delayMinutes > 1440;
  const input = toInput(form, rule?.rule.position);
  const overlaps = findAutomationRuleOverlaps(input, rules, rule?.rule.id);

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      title={rule ? t('automation.editor.edit_title') : t('automation.editor.create_title')}
      subtitle={t('automation.editor.subtitle')}
      footer={(
        <>
          <ChromeButton onClick={() => onOpenChange(false)}>{t('actions.cancel')}</ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={invalid || saving}
            onClick={() => onSubmit(input)}
          >
            {t('automation.actions.save_activate')}
          </ChromeButton>
        </>
      )}
    >
      <FormSection title={t('automation.editor.match_title')} description={t('automation.editor.match_hint')}>
        {overlaps.length > 0 && (
          <div className="flex gap-3 rounded-md border border-yellow/30 bg-[var(--yellow-tint)] px-3 py-2.5 text-xs text-[var(--yellow-ink)]">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            <div>
              <div className="font-strong">{t('automation.overlap.title')}</div>
              <div className="mt-1 leading-relaxed text-tx-2">
                {t('automation.overlap.description', {
                  rules: overlaps.slice(0, 3).map(({ rule: item }) => item.rule.name).join(', '),
                  count: overlaps.length,
                })}
              </div>
            </div>
          </div>
        )}
        <FormField label={t('fields.name')} required>
          <FormInput value={form.name} maxLength={255} onChange={(event) => setForm({ ...form, name: event.currentTarget.value })} />
        </FormField>
        <FormRow className="grid-cols-1 sm:grid-cols-2">
          <FormField label={t('automation.fields.source_kind')} required>
            <FormSelect
              value={form.sourceKind}
              onChange={(value) => {
                const sourceKind = value as AutomationSourceKind;
                setForm({
                  ...form,
                  sourceKind,
                  sourceId: '',
                  mode: sourceKind === 'synthetic_monitor' ? 'automatic' : 'approval',
                  delayMinutes: sourceKind === 'synthetic_monitor' ? '0' : '5',
                  // A title still at the default follows the source it names.
                  title: form.title === defaultTitle(form.sourceKind) ? defaultTitle(sourceKind) : form.title,
                });
              }}
              options={[
                { value: 'alert_incident', label: t('automation.source.alert_incident') },
                { value: 'synthetic_monitor', label: t('automation.source.synthetic_monitor') },
                { value: 'component_dependencies', label: t('automation.source.component_dependencies') },
              ]}
            />
          </FormField>
          <FormField label={t('automation.fields.minimum_severity')}>
            <FormSelect
              value={form.minimumSeverity}
              onChange={(value) => setForm({ ...form, minimumSeverity: value as AlertSeverity | '' })}
              options={[
                { value: '', label: t('values.any') },
                ...(['info', 'warning', 'error', 'critical'] as const).map((severity) => ({
                  value: severity,
                  label: t(`automation.severity.${severity}`),
                })),
              ]}
            />
          </FormField>
        </FormRow>
        <SourceResourceField
          sourceKind={form.sourceKind}
          value={form.sourceId}
          onChange={(sourceId) => setForm({ ...form, sourceId })}
          enabled={open}
          components={components}
        />
        <FormField label={t('automation.fields.labels')} hint={t('automation.fields.labels_hint')}>
          <FormInput value={form.labels} placeholder="service=checkout, env=production" onChange={(event) => setForm({ ...form, labels: event.currentTarget.value })} />
        </FormField>
      </FormSection>

      <FormSection title={t('automation.editor.publication_title')} description={t('automation.editor.publication_hint')}>
        <FormRow className="grid-cols-1 sm:grid-cols-2">
          <FormField label={t('automation.fields.mode')}>
            <FormSelect
              value={form.mode}
              onChange={(value) => setForm({ ...form, mode: value as AutomationPublicationMode })}
              options={[
                { value: 'approval', label: t('automation.mode.approval') },
                { value: 'automatic', label: t('automation.mode.automatic') },
              ]}
            />
          </FormField>
          <FormField label={t('automation.fields.delay_minutes')}>
            <FormInput type="number" min={0} max={1440} value={form.delayMinutes} onChange={(event) => setForm({ ...form, delayMinutes: event.currentTarget.value })} />
          </FormField>
        </FormRow>
        <FormRow className="grid-cols-1 sm:grid-cols-2">
          <ImpactField label={t('automation.fields.degraded_impact')} value={form.degradedImpact} onChange={(degradedImpact) => setForm({ ...form, degradedImpact })} />
          <ImpactField label={t('automation.fields.failing_impact')} value={form.failingImpact} onChange={(failingImpact) => setForm({ ...form, failingImpact })} />
        </FormRow>
        <FormField
          label={t('fields.affected_components')}
          required={!componentsOptional}
          {...(componentsOptional ? { hint: t('automation.fields.dependency_components_hint') } : {})}
        >
          <div className="grid gap-2 sm:grid-cols-2">
            {activeComponents.map((component) => (
              <label key={component.id} className="flex min-h-10 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 px-3 text-xs text-tx-1 hover:bg-bg-3">
                <input
                  type="checkbox"
                  checked={form.componentIds.includes(component.id)}
                  onChange={(event) => setForm({
                    ...form,
                    componentIds: event.currentTarget.checked
                      ? [...form.componentIds, component.id]
                      : form.componentIds.filter((id) => id !== component.id),
                  })}
                  className="h-4 w-4 accent-indigo"
                />
                <span>{component.name}</span>
              </label>
            ))}
          </div>
        </FormField>
      </FormSection>

      <FormSection title={t('automation.editor.copy_title')} description={t('automation.editor.copy_hint')}>
        <FormField label={t('automation.fields.title_template')} required>
          <FormInput value={form.title} maxLength={200} onChange={(event) => setForm({ ...form, title: event.currentTarget.value })} />
        </FormField>
        <FormField label={t('automation.fields.investigating_template')} required>
          <FormTextarea value={form.investigating} maxLength={4000} onChange={(event) => setForm({ ...form, investigating: event.currentTarget.value })} />
        </FormField>
        <FormField label={t('automation.fields.update_template')} required>
          <FormTextarea value={form.update} maxLength={4000} onChange={(event) => setForm({ ...form, update: event.currentTarget.value })} />
        </FormField>
        <FormField label={t('automation.fields.resolved_template')} required>
          <FormTextarea value={form.resolved} maxLength={4000} onChange={(event) => setForm({ ...form, resolved: event.currentTarget.value })} />
        </FormField>
        <FormField label={t('automation.fields.correlation_template')} hint={t('automation.fields.template_hint')} required>
          <FormInput value={form.correlation} maxLength={255} onChange={(event) => setForm({ ...form, correlation: event.currentTarget.value })} />
        </FormField>
      </FormSection>
    </FormDrawer>
  );
}

function ImpactField({ label, value, onChange }: { label: string; value: IncidentImpact; onChange: (value: IncidentImpact) => void }) {
  const { t } = useTranslation('status-pages');
  return (
    <FormField label={label}>
      <FormSelect value={value} onChange={(next) => onChange(next as IncidentImpact)} options={(['minor', 'major', 'critical'] as const).map((impact) => ({ value: impact, label: t(`impact.${impact}`) }))} />
    </FormField>
  );
}

interface EditorState {
  name: string;
  sourceKind: AutomationSourceKind;
  sourceId: string;
  minimumSeverity: AlertSeverity | '';
  labels: string;
  mode: AutomationPublicationMode;
  delayMinutes: string;
  degradedImpact: IncidentImpact;
  failingImpact: IncidentImpact;
  componentIds: string[];
  title: string;
  investigating: string;
  update: string;
  resolved: string;
  correlation: string;
}

function editorState(rule: ActiveAutomationRule | null): EditorState {
  const revision = rule?.revision;
  return {
    name: rule?.rule.name ?? '',
    sourceKind: revision?.matchers.sourceKind ?? 'alert_incident',
    sourceId: revision?.matchers.sourceId ?? '',
    minimumSeverity: revision?.matchers.minimumSeverity ?? '',
    labels: Object.entries(revision?.matchers.labels ?? {}).map(([key, value]) => `${key}=${value}`).join(', '),
    mode: revision?.action.publicationMode ?? 'approval',
    delayMinutes: String((revision?.action.sustainedDelaySeconds ?? 300) / 60),
    degradedImpact: revision?.action.impactMap.degraded ?? 'minor',
    failingImpact: revision?.action.impactMap.failing ?? 'major',
    componentIds: revision?.action.componentIds ?? [],
    title: revision?.action.templates.title ?? defaultTitle('alert_incident'),
    investigating: revision?.action.templates.investigating ?? 'We are investigating reports of degraded service.',
    update: revision?.action.templates.update ?? 'We have confirmed an increased impact and are continuing remediation.',
    resolved: revision?.action.templates.resolved ?? 'The service has recovered and is operating normally.',
    correlation: revision?.action.correlationKeyTemplate ?? '{{source.id}}',
  };
}

function defaultTitle(sourceKind: AutomationSourceKind): string {
  return sourceKind === 'component_dependencies' ? '{{labels.component_name}}' : '{{labels.incident_summary}}';
}

function toInput(form: EditorState, position?: number): AutomationRuleInput {
  const delayMinutes = Number(form.delayMinutes);
  const labels = Object.fromEntries(form.labels.split(',').map((item) => item.trim()).filter(Boolean).map((item) => {
    const [key = '', ...value] = item.split('=');
    return [key.trim(), value.join('=').trim()];
  }).filter(([key, value]) => key && value));
  return {
    name: form.name.trim(),
    ...(position === undefined ? {} : { position }),
    matchers: {
      sourceKind: form.sourceKind,
      sourceId: form.sourceId.trim() || null,
      labels,
      minimumSeverity: form.minimumSeverity || null,
    },
    action: {
      componentIds: form.componentIds,
      publicationMode: form.mode,
      sustainedDelaySeconds: Number.isFinite(delayMinutes)
        ? Math.max(0, Math.min(86_400, Math.round(delayMinutes * 60)))
        : 0,
      impactMap: { degraded: form.degradedImpact, failing: form.failingImpact },
      templates: { title: form.title.trim(), investigating: form.investigating.trim(), update: form.update.trim(), resolved: form.resolved.trim() },
      correlationKeyTemplate: form.correlation.trim(),
    },
  };
}
