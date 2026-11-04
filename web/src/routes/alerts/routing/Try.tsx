import { useMutation } from '@tanstack/react-query';
import { Check } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { IncidentOrigin, RoutingExplanation, RoutingRule } from '@/api/alertRouting';
import * as routingApi from '@/api/alertRouting';
import { INCIDENT_ORIGINS } from '@/api/alertRouting';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { DateTimePicker } from '@/shell/DateTimePicker';
import { FormField, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { prioritiesOf, priorityLabel, useImportance } from '@/shell/incident/Importance';
import { routingReason, serviceName } from '@/shell/incident/Routing';
import { cn } from '@/shell/lib/cn';
import type { Priority, Severity } from '@/types/alerting';

import { DEFAULT_POLICY, labelsFromText } from './model';
import { localInputToMicros } from '../schedule/EditorDrawer';
import { SECTION_INSET, SourceSection } from '../sources/Section';

const SEVERITIES: readonly Severity[] = ['info', 'warning', 'error', 'critical'];

/** The label an incident's origin is told by, as the backend reads it. */
const ORIGIN_LABEL: Record<IncidentOrigin, string | null> = {
  rule: null,
  external: 'external',
  synthetic: 'synthetic',
  manual: 'manual',
};

type StepState = 'used' | 'passed';

/**
 * Where an incident with these labels would go, and what each step of routing saw on the way:
 * the rules, the owner of its service, its team label, the default policy.
 */
export function TryRouting({
  rules,
  policies,
  teams,
}: {
  rules: readonly RoutingRule[];
  policies: ReadonlyArray<{ id: string; name: string }>;
  teams: ReadonlyArray<{ id: string; name: string }>;
}) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const priorities = prioritiesOf(levels);
  const [labels, setLabels] = React.useState('');
  const [origin, setOrigin] = React.useState<IncidentOrigin>('rule');
  const [severity, setSeverity] = React.useState<Severity>('error');
  const [priority, setPriority] = React.useState<Priority | ''>('');
  // Empty tries the rules as they stand now; a moment tries their time conditions then.
  const [at, setAt] = React.useState('');

  const run = useMutation({
    mutationFn: () => {
      const typed = labelsFromText(labels);
      const source = ORIGIN_LABEL[origin];

      return routingApi.test({
        labels: source ? { ...typed, 'alert.source': source } : typed,
        severity,
        ...(priority ? { priority } : {}),
        ...(at ? { at: localInputToMicros(at) } : {}),
      });
    },
  });

  return (
    <SourceSection title={t('routing.try_title')} description={t('routing.try_description')}>
      <form
        className={cn('grid gap-[16px] pb-[20px] lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]', SECTION_INSET)}
        onSubmit={(event) => {
          event.preventDefault();
          run.mutate();
        }}
      >
        <div className="flex min-w-0 flex-col gap-3">
          <FormField label={t('routing.try_labels')} hint={t('routing.try_labels_hint')}>
            <FormTextarea
              value={labels}
              onChange={(event) => setLabels(event.target.value)}
              rows={5}
              spellCheck={false}
              placeholder={'service.name=checkout\nenv=prod'}
              className="font-mono text-xs"
            />
          </FormField>
          <div className="grid grid-cols-2 gap-3">
            <FormField label={t('routing.try_origin')}>
              <FormSelect
                value={origin}
                onChange={(value) => setOrigin(value as IncidentOrigin)}
                options={INCIDENT_ORIGINS.map((value) => ({ value, label: t(`routing.origins.${value}`) }))}
              />
            </FormField>
            <FormField label={t('routing.try_severity')}>
              <FormSelect
                value={severity}
                onChange={(value) => setSeverity(value as Severity)}
                options={SEVERITIES.map((value) => ({ value, label: t(`severity.${value}`) }))}
              />
            </FormField>
            <FormField label={t('routing.try_priority')}>
              <FormSelect
                value={priority}
                onChange={(value) => setPriority(value as Priority | '')}
                options={[
                  { value: '', label: t('importance.without_priority') },
                  ...priorities.map((value) => ({ value, label: priorityLabel(levels, value) })),
                ]}
              />
            </FormField>
            <FormField label={t('routing.try_at')}>
              <DateTimePicker
                value={at}
                onChange={setAt}
                placeholder={t('routing.try_at_now')}
                aria-label={t('routing.try_at')}
              />
            </FormField>
          </div>
          <div>
            <ChromeButton type="submit" disabled={run.isPending}>
              {t('routing.try_run')}
            </ChromeButton>
          </div>
        </div>
        <div className="min-w-0" aria-live="polite">
          {run.isError ? (
            <p className="m-0 font-sans text-sm text-[var(--red-ink)]">{toApiError(run.error).message}</p>
          ) : run.data ? (
            <Outcome explanation={run.data} rules={rules} policies={policies} teams={teams} />
          ) : (
            <p className="m-0 rounded-md bg-[var(--page-canvas)] px-3 py-2.5 font-sans text-sm text-tx-2">
              {t('routing.try_idle')}
            </p>
          )}
        </div>
      </form>
    </SourceSection>
  );
}

function Outcome({
  explanation,
  rules,
  policies,
  teams,
}: {
  explanation: RoutingExplanation;
  rules: readonly RoutingRule[];
  policies: ReadonlyArray<{ id: string; name: string }>;
  teams: ReadonlyArray<{ id: string; name: string }>;
}) {
  const { t } = useTranslation('alerts');
  const { decision, matchedRuleId: matchedId } = explanation;
  const service = explanation.service ? serviceName(explanation.service) : null;
  const teamName = (id: string | null | undefined) => teams.find((team) => team.id === id)?.name ?? id ?? '';
  const policy = decision.escalationPolicyId;
  const policyName =
    policy === DEFAULT_POLICY ? t('routing.default_policy') : (policies.find((item) => item.id === policy)?.name ?? policy);
  const matched = rules.find((rule) => rule.id === matchedId);
  const ruleTeam = matched?.target.kind === 'team' ? matched.target.teamId : null;
  // Where the team came from: a matched rule's own team first, then the owner of the service, then the label.
  const teamFrom = ruleTeam ? 'rule' : explanation.serviceTeamId ? 'service' : explanation.labelTeamId ? 'label' : null;
  const reason = decision.routing.reason;

  const steps: Array<{ key: string; title: string; finding: string; state: StepState }> = [
    {
      key: 'rule',
      title: t('routing.steps.rule'),
      finding: matched
        ? t('routing.steps.rule_matched', { name: matched.name })
        : rules.some((rule) => rule.enabled)
          ? t('routing.steps.rule_none')
          : t('routing.steps.rule_empty'),
      state: matched ? 'used' : 'passed',
    },
    {
      key: 'service',
      title: t('routing.steps.service'),
      finding: !service
        ? t('routing.steps.service_none')
        : explanation.serviceTeamId
          ? t('routing.steps.service_owned', { service, team: teamName(explanation.serviceTeamId) })
          : t('routing.steps.service_unowned', { service }),
      state: reason === 'service_owner' || teamFrom === 'service' ? 'used' : 'passed',
    },
    {
      key: 'label',
      title: t('routing.steps.label'),
      finding: explanation.labelTeamId
        ? t('routing.steps.label_team', { team: teamName(explanation.labelTeamId) })
        : t('routing.steps.label_none'),
      state: reason === 'team_label' || teamFrom === 'label' ? 'used' : 'passed',
    },
    {
      key: 'default',
      title: t('routing.steps.default'),
      finding:
        policy !== DEFAULT_POLICY
          ? t('routing.steps.default_unused')
          : decision.teamId
            ? t('routing.steps.default_for_team', { team: teamName(decision.teamId) })
            : t('routing.steps.default_used'),
      state: policy === DEFAULT_POLICY ? 'used' : 'passed',
    },
  ];

  return (
    <div data-routing-outcome={reason} className="flex min-w-0 flex-col gap-3">
      <dl className="m-0 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5 rounded-md bg-[var(--page-canvas)] px-3 py-2.5 font-sans text-sm">
        <dt className="text-tx-2">{t('routing.outcome_team')}</dt>
        <dd className="m-0 truncate font-strong text-tx-0" data-routing-outcome-team>
          {decision.teamId ? teamName(decision.teamId) : t('routing.no_team')}
        </dd>
        <dt className="text-tx-2">{t('routing.outcome_policy')}</dt>
        <dd className="m-0 truncate font-strong text-tx-0" data-routing-outcome-policy>
          {policyName}
        </dd>
        <dt className="text-tx-2">{t('routing.outcome_reason')}</dt>
        <dd className="m-0 truncate text-tx-1">{routingReason(t, decision.routing)}</dd>
      </dl>
      <ol className="m-0 flex list-none flex-col gap-2 p-0">
        {steps.map((step, index) => (
          <li key={step.key} data-routing-step={step.key} data-state={step.state} className="grid grid-cols-[20px_minmax(0,1fr)] gap-x-2.5">
            <span
              aria-hidden
              className={cn(
                'mt-px grid h-5 w-5 place-items-center rounded-full font-mono text-xs font-bold',
                step.state === 'used' ? 'bg-[var(--green-tint)] text-[var(--green-ink)]' : 'bg-bg-2 text-tx-3',
              )}
            >
              {step.state === 'used' ? <Check className="h-3 w-3 stroke-[3]" /> : index + 1}
            </span>
            <div className="min-w-0 font-sans text-sm">
              <span className={step.state === 'used' ? 'font-strong text-tx-0' : 'text-tx-1'}>{step.title}</span>
              <span className="block truncate text-xs text-tx-2">{step.finding}</span>
            </div>
          </li>
        ))}
      </ol>
    </div>
  );
}
