import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import { ValueCombobox } from '@/routes/rum/analytics/ValueCombobox';
import { FormField, FormSelect } from '@/shell/FormDrawer';
import { useAuthStore } from '@/stores/auth';
import type { RumAlertMetric, RumAlertStatistic } from '@/types/alerting';

import { RUM_STATISTICS } from './rumSource';

const DAY_MICROS = 86_400_000_000;

/** The names an application measured over the last day: its vitals, or the timings its views marked. */
function useCustomNames(application: string, metric: RumAlertMetric) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const [range] = React.useState(() => {
    const to = Date.now() * 1_000;

    return { from: to - DAY_MICROS, to };
  });

  return useQuery({
    queryKey: ['alerts', 'rum', 'custom-names', orgId, application, metric],
    queryFn: async ({ signal }) => {
      const filters = [{ field: 'application.id', op: '=' as const, value: application }];

      if (metric === 'custom_vital') {
        const found = await rumApi.facets({ type: 'vitals', from: range.from, to: range.to, filters, field: 'vital.name', limit: 50 }, signal);

        return found.values.map((entry) => entry.value);
      }

      const found = await rumApi.keys({ type: 'views', from: range.from, to: range.to, filters, column: 'view.custom_timings', limit: 50 }, signal);

      return found.keys.map((entry) => entry.key);
    },
    enabled: Boolean(orgId && application),
    staleTime: 60_000,
  });
}

/** Which figure a rule on what the application measures itself watches, and what it takes of it. */
export function RumCustomFigureFields({
  application,
  metric,
  name,
  statistic,
  onName,
  onStatistic,
}: {
  application: string;
  metric: RumAlertMetric;
  name: string;
  statistic: RumAlertStatistic;
  onName: (name: string) => void;
  onStatistic: (statistic: RumAlertStatistic) => void;
}) {
  const { t } = useTranslation('alerts');
  const names = useCustomNames(application, metric);
  const label = t(metric === 'custom_vital' ? 'workbench.rum.custom.name_vital' : 'workbench.rum.custom.name_timing');

  return (
    <>
      <FormField label={label} required>
        <ValueCombobox value={name} onChange={onName} suggestions={names.data ?? []} placeholder={t('workbench.rum.custom.placeholder')} label={label} />
      </FormField>
      <FormField label={t('workbench.rum.custom.statistic')}>
        <FormSelect
          ariaLabel={t('workbench.rum.custom.statistic')}
          value={statistic}
          onChange={(value) => onStatistic(value as RumAlertStatistic)}
          options={RUM_STATISTICS.map((option) => ({ value: option, label: t(`workbench.rum.custom.statistics.${option}`) }))}
        />
      </FormField>
    </>
  );
}
