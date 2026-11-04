import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ComponentDependency, DependencyImpact, DependencyKind } from '@/api/statusPages';
import { FormInput, FormSelect } from '@/shell/FormDrawer';

import { DEPENDENCY_KINDS } from './model';
import type { DependencyTargetOption } from './targets';

/** One thing the component depends on: what kind of thing, which one, and how much of its trouble passes on. */
export function DependencyRow({
  dependency,
  onChange,
  options,
}: {
  dependency: ComponentDependency;
  onChange: (dependency: ComponentDependency) => void;
  options: (kind: DependencyKind) => DependencyTargetOption[];
}) {
  const { t } = useTranslation('status-pages');
  const listId = React.useId();
  const choices = options(dependency.kind);
  const missing = dependency.target !== ''
    && dependency.kind !== 'technical_service'
    && !choices.some((option) => option.value === dependency.target);

  return (
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-[150px_minmax(0,1fr)_150px]">
      <FormSelect
        ariaLabel={t('dependencies.kind')}
        value={dependency.kind}
        onChange={(value) => onChange({ ...dependency, kind: value as DependencyKind, target: '' })}
        options={DEPENDENCY_KINDS.map((kind) => ({ value: kind, label: t(`dependencies.kinds.${kind}`) }))}
      />
      {dependency.kind === 'technical_service' ? (
        <>
          <FormInput
            aria-label={t('dependencies.target')}
            value={dependency.target}
            maxLength={255}
            list={listId}
            placeholder={t('dependencies.technical_service_placeholder')}
            onChange={(event) => onChange({ ...dependency, target: event.currentTarget.value })}
          />
          <datalist id={listId}>
            {choices.map((option) => <option key={option.value} value={option.value} />)}
          </datalist>
        </>
      ) : (
        <FormSelect
          ariaLabel={t('dependencies.target')}
          value={dependency.target}
          placeholder={t(`dependencies.select_${dependency.kind}`)}
          onChange={(value) => onChange({ ...dependency, target: value })}
          options={[
            ...(missing ? [{ value: dependency.target, label: t('dependencies.unavailable_target', { id: dependency.target }) }] : []),
            ...choices,
          ]}
        />
      )}
      <FormSelect
        ariaLabel={t('dependencies.impact')}
        value={dependency.impact}
        onChange={(value) => onChange({ ...dependency, impact: value as DependencyImpact })}
        options={(['outage', 'degraded'] as const).map((impact) => ({
          value: impact,
          label: t(`dependencies.impacts.${impact}`),
        }))}
      />
    </div>
  );
}
