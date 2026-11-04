import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as alertSourcesApi from '@/api/alertSources';
import { FormField, FormSelect } from '@/shell/FormDrawer';

/** Every cluster counts and decides by itself. */
const EACH = 'each';

/**
 * Where a real-time rule is decided in a federation. Left on "each cluster", every cluster counts
 * and opens incidents by itself; set to one cluster, only that cluster opens them, adding the
 * counts of the other clusters the organization is mapped to. Hidden without a federation.
 */
export function HomeClusterField({
  value,
  onChange,
}: {
  value: string | null;
  onChange: (value: string | null) => void;
}) {
  const { t } = useTranslation('alerts');
  const clustersQuery = useQuery({
    queryKey: ['alert-sources', 'clusters'],
    queryFn: () => alertSourcesApi.clusters(),
    staleTime: 60_000,
  });
  const clusters = clustersQuery.data?.clusters ?? [];

  if (clusters.length === 0) return null;

  const known = value === null || clusters.some((cluster) => cluster.id === value);

  return (
    <FormField label={t('workbench.fields.home_cluster')} hint={t('workbench.hints.home_cluster')}>
      <FormSelect
        value={value ?? EACH}
        onChange={(next) => {
          // Radix reports '' when a value and its option arrive in one render; nobody picks nothing.
          if (next) onChange(next === EACH ? null : next);
        }}
        options={[
          { value: EACH, label: t('workbench.home_cluster.each') },
          ...clusters.map((cluster) => ({
            value: cluster.id,
            label: cluster.local ? t('workbench.home_cluster.local', { name: cluster.name }) : cluster.name,
          })),
          ...(known || value === null ? [] : [{ value, label: value }]),
        ]}
      />
    </FormField>
  );
}
