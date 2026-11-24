import { useTranslation } from 'react-i18next';

import { FormField, FormInput } from '@/shell/FormDrawer';

import type { ChangeSettings } from './model';

/**
 * Where a `to_change` step takes the environment of the changes it notes:
 * one set for every change, or a field of each event such as the cluster.
 */
export function ChangeFields({ change, onChange }: { change: ChangeSettings; onChange: (change: ChangeSettings) => void }) {
  const { t } = useTranslation('pipelines');
  const set = (patch: Partial<ChangeSettings>) => onChange({ ...change, ...patch });

  return (
    <div className="grid gap-x-[16px] gap-y-[12px] sm:grid-cols-2">
      <FormField label={t('counting.change_environment')} hint={t('counting.change_environment_hint')}>
        <FormInput
          value={change.environment}
          onChange={(event) => set({ environment: event.target.value })}
          placeholder="production"
          className="font-mono text-xs"
          autoComplete="off"
        />
      </FormField>
      <FormField label={t('counting.change_environment_field')} hint={t('counting.change_environment_field_hint')}>
        <FormInput
          value={change.environmentField}
          onChange={(event) => set({ environmentField: event.target.value })}
          placeholder="k8s.cluster.name"
          className="font-mono text-xs"
          autoComplete="off"
        />
      </FormField>
    </div>
  );
}
