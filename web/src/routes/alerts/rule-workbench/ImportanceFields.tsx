import { useTranslation } from 'react-i18next';

import { FormField, FormSelect } from '@/shell/FormDrawer';
import { prioritiesOf, priorityLabel, urgenciesOf, urgencyLabel, useImportance } from '@/shell/incident/Importance';
import type { Priority, Urgency } from '@/types/alerting';

/** The select value that leaves a field unset; not '', which Radix reports when a value and its
 * option arrive in one render (a rule at a level the organization's levels bring in later). */
const UNSET = '__unset';

/**
 * The priority and urgency the rule's incidents open with. Left unset, an incident has no priority
 * (an alert pipeline may still give it one) and the escalation policy's urgency rule decides.
 */
export function ImportanceFields({
  priority,
  urgency,
  onPriority,
  onUrgency,
}: {
  priority: Priority | null;
  urgency: Urgency | null;
  onPriority: (priority: Priority | null) => void;
  onUrgency: (urgency: Urgency | null) => void;
}) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const priorities = prioritiesOf(levels);
  const urgencies = urgenciesOf(levels);

  return (
    <div className="grid gap-3 sm:grid-cols-2" data-rule-importance>
      <FormField label={t('workbench.fields.priority')} hint={t('workbench.hints.priority')}>
        <FormSelect
          ariaLabel={t('workbench.fields.priority')}
          value={priority ?? UNSET}
          onChange={(next) => {
            if (next) onPriority(next === UNSET ? null : (next as Priority));
          }}
          options={[
            { value: UNSET, label: t('importance.without_priority') },
            ...priorities.map((value) => ({ value, label: priorityLabel(levels, value) })),
            // A level the organization dropped since stays readable until it is changed.
            ...(priority && !priorities.includes(priority) ? [{ value: priority, label: priority }] : []),
          ]}
        />
      </FormField>
      <FormField label={t('workbench.fields.urgency')} hint={t('workbench.hints.urgency')}>
        <FormSelect
          ariaLabel={t('workbench.fields.urgency')}
          value={urgency ?? UNSET}
          onChange={(next) => {
            if (next) onUrgency(next === UNSET ? null : (next as Urgency));
          }}
          options={[
            { value: UNSET, label: t('workbench.urgency_by_policy') },
            ...urgencies.map((value) => ({ value, label: urgencyLabel(levels, value, t) })),
            ...(urgency && !urgencies.includes(urgency) ? [{ value: urgency, label: urgency }] : []),
          ]}
        />
      </FormField>
    </div>
  );
}
