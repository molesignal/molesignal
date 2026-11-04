import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import type { AlertSource } from '@/api/alertSources';
import * as escalationsApi from '@/api/escalations';

/** The name of the escalation a source pages through, as a row says it. */
export function usePolicyName() {
  const { t } = useTranslation('alerts');
  const query = useQuery({ queryKey: ['escalation-policies'], queryFn: () => escalationsApi.list() });

  return (source: Pick<AlertSource, 'escalationPolicyId'>) => {
    const id = source.escalationPolicyId;

    if (!id) return t('alert_sources.policy_none');
    if (id === 'default') return t('alert_sources.policy_default');
    return query.data?.find((policy) => policy.id === id)?.name ?? id;
  };
}
