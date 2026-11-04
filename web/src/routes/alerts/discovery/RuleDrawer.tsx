import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as alertsApi from '@/api/alerts';
import { FINDING_KINDS, type Finding, type FindingKind } from '@/api/discovery';
import * as escalationsApi from '@/api/escalations';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormGroup,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { urgenciesOf, urgencyLabel, useImportance } from '@/shell/incident/Importance';
import { toast } from '@/shell/ui/sonner';
import type { AlertRule, Severity, Urgency } from '@/types/alerting';

import { subjectName } from './model';
import { valuesFromText } from '../routing/model';

const SEVERITIES: readonly Severity[] = ['info', 'warning', 'error', 'critical'];

/** The policy alias every rule starts with until one is chosen. */
const DEFAULT_POLICY = 'default';

/**
 * A rule whose source is automatic discovery: which findings it takes, and who is told. Each
 * finding it takes opens its own incident with the finding's severity, low urgency unless set,
 * and the incident recovers when the finding ends.
 */
export function DiscoveryRuleDrawer({
  open,
  from,
  editing,
  onClose,
}: {
  open: boolean;
  /** The finding the rule starts from: its kind and its service are taken. */
  from?: Finding | null;
  /** A discovery rule to change. */
  editing?: AlertRule | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const urgencies = urgenciesOf(levels);
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'alerts.rules.manage' });
  const policies = useQuery({ queryKey: ['escalation-policies'], queryFn: () => escalationsApi.list(), enabled: open });
  const [name, setName] = React.useState('');
  const [kinds, setKinds] = React.useState<FindingKind[]>([]);
  const [subjects, setSubjects] = React.useState('');
  const [environments, setEnvironments] = React.useState('');
  const [minSeverity, setMinSeverity] = React.useState<Severity | ''>('');
  const [urgency, setUrgency] = React.useState<Urgency | ''>('');
  const [policy, setPolicy] = React.useState(DEFAULT_POLICY);

  React.useEffect(() => {
    if (!open) return;
    const query = editing?.query.discovery;
    setName(editing?.name ?? (from ? t('discovery.rule.name_from', { subject: subjectName(from) }) : ''));
    setKinds(query?.kinds ?? (from ? [from.kind] : []));
    setSubjects((query?.subjects ?? (from ? [from.subject] : [])).join(', '));
    setEnvironments((query?.environments ?? (from?.environment ? [from.environment] : [])).join(', '));
    setMinSeverity(query?.minSeverity ?? '');
    setUrgency(editing?.trigger.urgency ?? '');
    setPolicy(editing?.escalationPolicyId ?? DEFAULT_POLICY);
  }, [open, editing, from, t]);

  const save = useMutation({
    mutationFn: () => {
      const input: alertsApi.AlertRuleInput = {
        name: name.trim(),
        description: editing?.description ?? '',
        enabled: editing?.enabled ?? true,
        kind: 'scheduled',
        // The server gives a discovery rule its one shape: split by finding, graded by its severity.
        query: {
          language: 'sql',
          statement: 'discovery',
          periodSecs: 60,
          discovery: {
            kinds,
            subjects: valuesFromText('in', subjects),
            environments: valuesFromText('in', environments),
            minSeverity: minSeverity || null,
          },
        },
        trigger: {
          operator: 'gte',
          threshold: 1,
          forPeriods: 1,
          silenceSecs: editing?.trigger.silenceSecs ?? 0,
          ...(urgency ? { urgency } : {}),
          ...(editing?.trigger.priority ? { priority: editing.trigger.priority } : {}),
        },
        thresholds: [],
        severity: null,
        escalationPolicyId: policy,
        labels: editing?.labels ?? {},
        annotations: editing?.annotations ?? {},
      };
      return editing ? alertsApi.update(editing.id, input) : alertsApi.create(input);
    },
    onSuccess: () => {
      toast.success(t(editing ? 'discovery.rule.updated' : 'discovery.rule.created'));
      void queryClient.invalidateQueries({ queryKey: ['alerts', 'rules'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const invalid = !name.trim();

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t(editing ? 'discovery.rule.edit_title' : 'discovery.rule.new_title')}
      subtitle={t('discovery.rule.subtitle')}
      footer={
        <FormSubmitFooter
          formId="discovery-rule-form"
          busy={save.isPending}
          disabled={access.disabled}
          disabledReason={access.reason}
          invalid={invalid}
          onCancel={onClose}
          submitLabel={t(editing ? 'discovery.rule.save' : 'discovery.rule.create')}
        />
      }
    >
      <form
        id="discovery-rule-form"
        data-discovery-rule
        onSubmit={(event) => {
          event.preventDefault();
          if (!access.allowed || invalid) return;
          save.mutate();
        }}
      >
        <FormSection title={t('discovery.rule.sections.which')}>
          <FormField label={t('discovery.rule.fields.name')} required>
            <FormInput value={name} onChange={(event) => setName(event.target.value)} required />
          </FormField>
          <FormGroup label={t('discovery.rule.fields.kinds')} hint={t('discovery.rule.hints.kinds')}>
            <FormChecklist
              selected={kinds}
              onChange={setKinds}
              options={FINDING_KINDS.map((kind) => ({ value: kind, label: t(`discovery.kinds.${kind}`) }))}
            />
          </FormGroup>
          <div className="grid gap-3 sm:grid-cols-2">
            <FormField label={t('discovery.rule.fields.subjects')} hint={t('discovery.rule.hints.subjects')}>
              <FormInput value={subjects} onChange={(event) => setSubjects(event.target.value)} placeholder="checkout, web-shop" />
            </FormField>
            <FormField label={t('discovery.rule.fields.environments')} hint={t('discovery.rule.hints.environments')}>
              <FormInput value={environments} onChange={(event) => setEnvironments(event.target.value)} placeholder="prod" />
            </FormField>
          </div>
          <FormField label={t('discovery.rule.fields.min_severity')}>
            <FormSelect
              ariaLabel={t('discovery.rule.fields.min_severity')}
              value={minSeverity}
              onChange={(next) => setMinSeverity(next as Severity | '')}
              options={[
                { value: '', label: t('routing.drawer.any_severity') },
                ...SEVERITIES.map((severity) => ({ value: severity, label: t(`severity.${severity}`) })),
              ]}
            />
          </FormField>
        </FormSection>
        <FormSection title={t('discovery.rule.sections.who')} className="mb-0">
          <div className="grid gap-3 sm:grid-cols-2">
            <FormField label={t('discovery.rule.fields.policy')}>
              <FormSelect
                ariaLabel={t('discovery.rule.fields.policy')}
                value={policy}
                onChange={setPolicy}
                options={[
                  { value: DEFAULT_POLICY, label: t('routing.default_policy') },
                  ...(policies.data ?? [])
                    .filter((item) => item.id !== DEFAULT_POLICY)
                    .map((item) => ({ value: item.id, label: item.name })),
                ]}
              />
            </FormField>
            <FormField label={t('discovery.rule.fields.urgency')} hint={t('discovery.rule.hints.urgency')}>
              <FormSelect
                ariaLabel={t('discovery.rule.fields.urgency')}
                value={urgency}
                onChange={(next) => setUrgency(next as Urgency | '')}
                options={[
                  { value: '', label: t('discovery.rule.urgency_low_default') },
                  ...urgencies.map((value) => ({ value, label: urgencyLabel(levels, value, t) })),
                ]}
              />
            </FormField>
          </div>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
