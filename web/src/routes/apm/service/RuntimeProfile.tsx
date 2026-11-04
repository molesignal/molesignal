import { useQuery } from '@tanstack/react-query';
import { Flame } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as profilesApi from '@/api/profiles';
import { profileTypesIn, useProfileTypeLabel } from '@/routes/profiles/shared';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Flamegraph } from '@/viz/profiles/Flamegraph';

import { signalHref } from './links';
import type { ServiceAddress } from './useService';
import { ApmCard, ApmQueryState } from '../frame/Page';

/** As many profiles as one look at the range lists: enough to say which types there are. */
const LIST_LIMIT = 200;

/**
 * Where the service's processes spend their time and memory, from the
 * profiles it sent within the range: one flame graph of one type at a time
 * (CPU, wall clock, allocations, heap, locks, goroutines), since seconds and
 * bytes do not add up, the types it has data of to choose from.
 */
export function RuntimeProfile({ address }: { address: ServiceAddress }) {
  const { t } = useTranslation('apm');
  const typeLabel = useProfileTypeLabel();
  const from = address.range.from.getTime() * 1_000;
  const to = address.range.to.getTime() * 1_000;
  const [chosen, setChosen] = React.useState('');

  const listed = useQuery({
    queryKey: ['apm', 'runtime-profiles', address.orgId, address.name, from, to],
    queryFn: () => profilesApi.list({ service: address.name, from, to, limit: LIST_LIMIT }),
    enabled: Boolean(address.orgId && address.name),
    staleTime: 60_000,
  });
  const types = React.useMemo(() => profileTypesIn(listed.data ?? []), [listed.data]);
  const type = chosen && types.includes(chosen) ? chosen : (types[0] ?? '');

  const flame = useQuery({
    queryKey: ['apm', 'runtime-flame', address.orgId, address.name, type, from, to],
    queryFn: () => profilesApi.flamegraph({ service: address.name, type, from, to }),
    enabled: Boolean(address.orgId && type),
    staleTime: 60_000,
  });
  const handle = { namespace: address.namespace, service: address.name, environment: address.environment, from, to };

  return (
    <ApmCard
      icon={Flame}
      tone="text-[var(--orange-ink)]"
      title={t('runtime.profile.title')}
      description={t('runtime.profile.description')}
      testId="runtime-profile"
      actions={
        types.length > 0 ? (
          <Link to={signalHref('profiles', handle)} className="font-sans text-xs font-strong text-tx-2 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none">
            {t('runtime.profile.open')}
          </Link>
        ) : undefined
      }
    >
      <ApmQueryState query={listed} empty={types.length === 0} emptyTitle={t('runtime.profile.none')} emptyDescription={t('runtime.profile.none_description')} compact>
        <div className="space-y-2 px-3 py-2">
          {types.length > 1 && (
            <ViewTabs label={t('runtime.profile.types')} value={type} onValueChange={setChosen} items={types.map((key) => ({ key, label: typeLabel(key) }))} />
          )}
          <ApmQueryState query={flame} empty={!flame.data || flame.data.flamebearer.numTicks === 0} emptyTitle={t('runtime.profile.empty')} compact>
            {flame.data && <Flamegraph flamebearer={flame.data.flamebearer} service={address.name} className="border-0" />}
          </ApmQueryState>
        </div>
      </ApmQueryState>
    </ApmCard>
  );
}
