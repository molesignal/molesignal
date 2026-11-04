import { useTranslation } from 'react-i18next';

import { FormField, FormRadio, FormSection, FormSelect } from '@/shell/FormDrawer';
import { urgencyLabel, useImportance } from '@/shell/incident/Importance';
import { Checkbox } from '@/shell/ui/checkbox';
import { WeeklyWindowsFields, officeHours } from '@/shell/weekly-ranges/WeeklyWindowsFields';
import type { Urgency, UrgencyRule } from '@/types/alerting';

type UrgencyMode = UrgencyRule['mode'];

const MODES: readonly UrgencyMode[] = ['by_severity', 'always', 'support_hours'];

/** The rule of one mode, keeping the support hours already written when the mode comes back. */
export function ruleOfMode(mode: UrgencyMode, current: UrgencyRule): UrgencyRule {
  if (mode === 'by_severity') return { mode };
  if (mode === 'always') return current.mode === 'always' ? current : { mode, urgency: 'high' };
  return current.mode === 'support_hours'
    ? current
    : { mode, hours: officeHours(), raiseAtStart: false, inside: 'high', outside: 'low' };
}

/** Whether every incident the rule sets wakes people up the whole policy: then no step holds anything back. */
export function alwaysEscalates(rule: UrgencyRule, escalating: (urgency: Urgency) => boolean): boolean {
  return rule.mode === 'always' && escalating(rule.urgency);
}

/**
 * How the policy sets the urgency of an incident nothing else set it for, and whether incidents of
 * a level that stops at the first step still climb the steps.
 */
export function UrgencyFields({
  rule,
  onRule,
  escalateLow,
  onEscalateLow,
}: {
  rule: UrgencyRule;
  onRule: (rule: UrgencyRule) => void;
  escalateLow: boolean;
  onEscalateLow: (on: boolean) => void;
}) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const options = levels.urgencies.map((level) => ({ value: level.key, label: urgencyLabel(levels, level.key, t) }));
  const escalating = (urgency: Urgency) => levels.urgencies.find((level) => level.key === urgency)?.escalates ?? true;

  return (
    <FormSection
      title={t('escalations.urgency.title')}
      description={t('escalations.urgency.description')}
      className="mb-0"
    >
      <div data-urgency-rule={rule.mode}>
        <FormRadio<UrgencyMode>
          value={rule.mode}
          onChange={(mode) => {
            // Radix reports '' when a value and its option arrive in one render (the organization's
            // levels load after the policy); nobody picks nothing.
            if (mode) onRule(ruleOfMode(mode, rule));
          }}
          options={MODES.map((mode) => ({
            value: mode,
            label: t(`escalations.urgency.modes.${mode}`),
            hint: t(`escalations.urgency.mode_hints.${mode}`),
          }))}
        />
      </div>
      {rule.mode === 'always' && (
        <FormField label={t('escalations.urgency.always_level')}>
          <FormSelect
            ariaLabel={t('escalations.urgency.always_level')}
            value={rule.urgency}
            onChange={(urgency) => {
              if (urgency) onRule({ ...rule, urgency });
            }}
            options={options}
          />
        </FormField>
      )}
      {rule.mode === 'support_hours' && (
        <div className="flex flex-col gap-2">
          <WeeklyWindowsFields windows={rule.hours} onChange={(hours) => onRule({ ...rule, hours })} />
          <div className="grid gap-3 sm:grid-cols-2">
            <FormField label={t('escalations.urgency.inside')}>
              <FormSelect
                ariaLabel={t('escalations.urgency.inside')}
                value={rule.inside ?? 'high'}
                onChange={(inside) => {
                  if (inside) onRule({ ...rule, inside });
                }}
                options={options}
              />
            </FormField>
            <FormField label={t('escalations.urgency.outside')}>
              <FormSelect
                ariaLabel={t('escalations.urgency.outside')}
                value={rule.outside ?? 'low'}
                onChange={(outside) => {
                  if (outside) onRule({ ...rule, outside });
                }}
                options={options}
              />
            </FormField>
          </div>
          <label className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
            <Checkbox
              checked={rule.raiseAtStart ?? false}
              onCheckedChange={(next) => onRule({ ...rule, raiseAtStart: next === true })}
              data-raise-at-start
            />
            <span>{t('escalations.urgency.raise_at_start')}</span>
          </label>
        </div>
      )}
      {!alwaysEscalates(rule, escalating) && (
        <label className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
          <Checkbox
            checked={escalateLow}
            onCheckedChange={(next) => onEscalateLow(next === true)}
            data-escalate-low-urgency
          />
          <span>{t('escalations.urgency.escalate_low')}</span>
        </label>
      )}
    </FormSection>
  );
}
