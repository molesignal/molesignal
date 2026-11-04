import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { AutomationTriggerKind } from '@/api/agent/automations';
import * as alertsApi from '@/api/alerts';
import { FINDING_KINDS } from '@/api/discovery';
import { hasPermission, useProductAccess } from '@/product/access';
import { FormChecklist, FormField, FormGroup, FormInput, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { TimezoneField } from '@/shell/TimezoneField';

import { CRON_PRESETS, SEVERITIES, TRIGGER_KINDS, cronLooksValid, type AutomationDraft } from './model';

/**
 * What starts an automation, and the settings of that one start: a cron in a
 * time zone, which new incidents, which new findings of automatic discovery, a
 * call to its API, or a person running it.
 */
export function TriggerFields({
  draft,
  onChange,
}: {
  draft: AutomationDraft;
  onChange: (patch: Partial<AutomationDraft>) => void;
}) {
  const { t } = useTranslation('agent');

  return (
    <>
      <FormField label={t('automations.editor.trigger')} hint={t(`automations.trigger_hint.${draft.kind}`)}>
        <FormSelect
          ariaLabel={t('automations.editor.trigger')}
          value={draft.kind}
          onChange={(kind) => onChange({ kind: kind as AutomationTriggerKind })}
          options={TRIGGER_KINDS.map((kind) => ({
            value: kind,
            label: t(`automations.trigger_kind.${kind}`),
          }))}
        />
      </FormField>
      {draft.kind === 'schedule' && <ScheduleFields draft={draft} onChange={onChange} />}
      {draft.kind === 'incident' && <IncidentFields draft={draft} onChange={onChange} />}
      {draft.kind === 'finding' && <FindingFields draft={draft} onChange={onChange} />}
    </>
  );
}

/** Which new findings start it: their kinds, the least severity, and the services, hosts or containers. */
function FindingFields({
  draft,
  onChange,
}: {
  draft: AutomationDraft;
  onChange: (patch: Partial<AutomationDraft>) => void;
}) {
  const { t } = useTranslation('agent');
  const { t: ta } = useTranslation('alerts');
  const [subjects, setSubjects] = React.useState(draft.subjects.join(', '));

  return (
    <>
      <FormGroup label={t('automations.editor.finding_kinds')} hint={t('automations.editor.finding_kinds_hint')}>
        <div className="max-h-48 overflow-auto rounded-md" data-finding-kinds>
          <FormChecklist
            options={FINDING_KINDS.map((kind) => ({ value: kind, label: ta(`discovery.kinds.${kind}`) }))}
            selected={draft.findingKinds}
            onChange={(findingKinds) => onChange({ findingKinds })}
          />
        </div>
      </FormGroup>
      <FormField label={t('automations.editor.min_severity')}>
        <FormSelect
          ariaLabel={t('automations.editor.min_severity')}
          value={draft.minSeverity}
          onChange={(minSeverity) => onChange({ minSeverity })}
          options={[
            { value: '', label: t('automations.editor.any_severity') },
            ...[...SEVERITIES].reverse().map((severity) => ({ value: severity, label: t(`automations.severity.${severity}`) })),
          ]}
        />
      </FormField>
      <FormField label={t('automations.editor.finding_subjects')} hint={t('automations.editor.finding_subjects_hint')}>
        <FormInput
          value={subjects}
          placeholder="checkout, db-1"
          onChange={(event) => {
            setSubjects(event.target.value);
            onChange({
              subjects: event.target.value
                .split(',')
                .map((subject) => subject.trim())
                .filter(Boolean),
            });
          }}
        />
      </FormField>
    </>
  );
}

function ScheduleFields({
  draft,
  onChange,
}: {
  draft: AutomationDraft;
  onChange: (patch: Partial<AutomationDraft>) => void;
}) {
  const { t } = useTranslation('agent');
  const invalid = draft.cron.trim() !== '' && !cronLooksValid(draft.cron);

  return (
    <>
      <div className="flex flex-wrap gap-1.5" role="group" aria-label={t('automations.editor.presets')}>
        {CRON_PRESETS.map((preset) => {
          const chosen = draft.cron.trim() === preset.cron;
          return (
            <button
              key={preset.key}
              type="button"
              aria-pressed={chosen}
              onClick={() => onChange({ cron: preset.cron })}
              className={cn(
                'inline-flex h-7 items-center rounded-md px-2.5 font-sans text-xs font-strong transition-colors focus-visible:outline-none',
                chosen ? 'bg-indigo text-white' : 'bg-bg-2 text-tx-1 hover:bg-bg-3 focus-visible:bg-bg-3',
              )}
            >
              {t(`automations.cron.${preset.key}`)}
            </button>
          );
        })}
      </div>
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
        <FormField
          label={t('automations.editor.cron')}
          hint={invalid ? t('automations.editor.cron_invalid') : t('automations.editor.cron_hint')}
        >
          <FormInput
            value={draft.cron}
            spellCheck={false}
            aria-invalid={invalid || undefined}
            placeholder="0 9 * * 1-5"
            onChange={(event) => onChange({ cron: event.target.value })}
          />
        </FormField>
        <div className="flex flex-col gap-1.5">
          <span className="type-label font-sans font-medium text-tx-2">{t('automations.editor.timezone')}</span>
          <TimezoneField
            label={t('automations.editor.timezone')}
            value={draft.timezone}
            onChange={(timezone) => onChange({ timezone })}
          />
        </div>
      </div>
    </>
  );
}

function IncidentFields({
  draft,
  onChange,
}: {
  draft: AutomationDraft;
  onChange: (patch: Partial<AutomationDraft>) => void;
}) {
  const { t } = useTranslation('agent');
  const access = useProductAccess();
  const mayReadRules = hasPermission('alerts.rules.read', access);
  const rules = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: alertsApi.list,
    enabled: mayReadRules,
    retry: false,
  });
  const [services, setServices] = React.useState(draft.services.join(', '));

  const ruleOptions = React.useMemo(() => {
    const known = (rules.data ?? []).map((rule) => ({ value: rule.id, label: rule.name }));
    // A rule the automation is limited to stays listed after it was removed.
    const missing = draft.ruleIds
      .filter((id) => !known.some((rule) => rule.value === id))
      .map((id) => ({ value: id, label: id }));
    return [...known, ...missing];
  }, [draft.ruleIds, rules.data]);

  return (
    <>
      <FormField label={t('automations.editor.severities')} hint={t('automations.editor.severities_hint')}>
        <div className="flex flex-wrap gap-1.5" role="group" aria-label={t('automations.editor.severities')}>
          {SEVERITIES.map((severity) => {
            const chosen = draft.severities.includes(severity);
            return (
              <button
                key={severity}
                type="button"
                aria-pressed={chosen}
                onClick={() =>
                  onChange({
                    severities: chosen
                      ? draft.severities.filter((value) => value !== severity)
                      : [...draft.severities, severity],
                  })
                }
                className={cn(
                  'inline-flex h-7 items-center rounded-md px-2.5 font-sans text-xs font-strong transition-colors focus-visible:outline-none',
                  chosen ? 'bg-indigo text-white' : 'bg-bg-2 text-tx-1 hover:bg-bg-3 focus-visible:bg-bg-3',
                )}
              >
                {t(`automations.severity.${severity}`)}
              </button>
            );
          })}
        </div>
      </FormField>
      {mayReadRules && ruleOptions.length > 0 && (
        <FormGroup label={t('automations.editor.rules')} hint={t('automations.editor.rules_hint')}>
          <div className="max-h-48 overflow-auto rounded-md">
            <FormChecklist
              options={ruleOptions}
              selected={draft.ruleIds}
              onChange={(ruleIds) => onChange({ ruleIds })}
            />
          </div>
        </FormGroup>
      )}
      <FormField label={t('automations.editor.services')} hint={t('automations.editor.services_hint')}>
        <FormInput
          value={services}
          placeholder="checkout, payments"
          onChange={(event) => {
            setServices(event.target.value);
            onChange({
              services: event.target.value
                .split(',')
                .map((service) => service.trim())
                .filter(Boolean),
            });
          }}
        />
      </FormField>
      <FormField label={t('automations.editor.labels')} hint={t('automations.editor.labels_hint')}>
        <FormTextarea
          value={draft.labels}
          rows={2}
          spellCheck={false}
          className="min-h-[56px] text-xs"
          placeholder={'env=prod'}
          onChange={(event) => onChange({ labels: event.target.value })}
        />
      </FormField>
    </>
  );
}
