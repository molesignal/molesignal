import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type {
  BusinessServiceDependency,
  BusinessServiceDependencyImpact,
  BusinessServiceDependencyKind,
} from '@/api/businessServices';
import { FormInput, FormSelect } from '@/shell/FormDrawer';

import { DEPENDENCY_KINDS } from '../model';
import type { DependencyTargetOption } from '../targets';

/** One thing the service depends on: what kind of thing, which one, and how much of its trouble passes on. */
export function DependencyRow({
  dependency,
  onChange,
  options,
}: {
  dependency: BusinessServiceDependency;
  onChange: (dependency: BusinessServiceDependency) => void;
  options: (kind: BusinessServiceDependencyKind) => DependencyTargetOption[];
}) {
  const { t } = useTranslation('business-services');
  const listId = React.useId();
  const choices = options(dependency.kind);
  const missing = dependency.target !== ''
    && dependency.kind !== 'technical_service'
    && !choices.some((option) => option.value === dependency.target);

  return (
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-[150px_minmax(0,1fr)_150px]">
      <FormSelect
        ariaLabel={t('fields.dependency_kind')}
        value={dependency.kind}
        onChange={(value) => onChange({ ...dependency, kind: value as BusinessServiceDependencyKind, target: '' })}
        options={DEPENDENCY_KINDS.map((kind) => ({ value: kind, label: t(`dependency_kinds.${kind}`) }))}
      />
      {dependency.kind === 'technical_service' ? (
        <>
          <FormInput
            aria-label={t('fields.dependency_target')}
            value={dependency.target}
            maxLength={255}
            list={listId}
            placeholder={t('editor.technical_service_placeholder')}
            onChange={(event) => onChange({ ...dependency, target: event.currentTarget.value })}
          />
          <datalist id={listId}>
            {choices.map((option) => <option key={option.value} value={option.value} />)}
          </datalist>
        </>
      ) : (
        <FormSelect
          ariaLabel={t('fields.dependency_target')}
          value={dependency.target}
          placeholder={t(`editor.select_${dependency.kind}`)}
          onChange={(value) => onChange({ ...dependency, target: value })}
          options={[
            ...(missing ? [{ value: dependency.target, label: t('editor.unavailable_target', { id: dependency.target }) }] : []),
            ...choices,
          ]}
        />
      )}
      <FormSelect
        ariaLabel={t('fields.dependency_impact')}
        value={dependency.impact}
        onChange={(value) => onChange({ ...dependency, impact: value as BusinessServiceDependencyImpact })}
        options={(['outage', 'degraded'] as const).map((impact) => ({
          value: impact,
          label: t(`dependency_impacts.${impact}`),
        }))}
      />
    </div>
  );
}
