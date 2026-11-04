import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys } from '@/api/apm';
import { type ApmSettings as Settings, DEFAULT_APM_SETTINGS } from '@/api/apmSettings';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

import { EnvironmentMapCard, NamingRulesCard } from './NamingCards';
import { useDraft } from './parts';
import { LimitsCard, SamplingCard } from './PlatformCards';
import { QualityCard } from './QualityCard';
import { AliasesCard, CatalogRulesCard } from './ServiceRulesCards';
import { ThresholdsCard } from './ThresholdsCard';
import { useApmSettingsQuery, useSaveApmSettings } from './useApmSettings';
import { ApmNavigation } from '../frame/Navigation';
import { ApmPage, ApmQueryState } from '../frame/Page';

const DAY_MILLIS = 86_400_000;

/**
 * What the organization sets for APM, a card at a time, each saved on its
 * own: how well the services were read over the day and what a setting
 * would read as one; the health lines, the default Apdex T and the long
 * task; what the catalog asks by tier; naming rules, environment mapping
 * and aliases. Whoever manages the platform also sets the cardinality
 * limits and the trace sampling policy here.
 */
export function ApmSettings() {
  const { t } = useTranslation('apm');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const query = useApmSettingsQuery();
  const save = useSaveApmSettings();
  const access = useActionAccess({ permission: 'apm.settings.manage' });
  const product = useProductAccess();
  const saved = query.data ?? DEFAULT_APM_SETTINGS;
  const naming = useDraft(saved.namingRules);
  const environments = useDraft(saved.environmentMap);
  const aliases = useDraft(saved.aliases);
  const catalogRules = useDraft(saved.metadataRules);
  // The day before the page opened: what the quality card reads.
  const [params] = React.useState(() => {
    const to = Date.now();
    return { from: (to - DAY_MILLIS) * 1_000, to: to * 1_000 };
  });
  const overview = useQuery({
    queryKey: apmQueryKeys.overview(orgId, params),
    queryFn: () => apmApi.overview(params),
    enabled: Boolean(orgId),
  });
  const services = [...new Set((overview.data?.services ?? []).map((summary) => summary.service.name))].sort();
  const limitsShown = hasPermission('sys.settings.limits.read', product);

  const persist = (patch: Partial<Settings>, done: () => void) =>
    save.mutate(
      { ...saved, ...patch },
      {
        onSuccess: () => {
          done();
          toast.success(t('org_settings.saved'));
        },
        onError: (error) => toast.error(toApiError(error).message),
      },
    );
  const shared = { access, pending: save.isPending };

  return (
    <ApmPage page="settings" title={t('title')} subtitle={t('subtitle')} breadcrumbs={null} navigation={<ApmNavigation />}>
      <ApmQueryState query={query} empty={false}>
        <QualityCard
          query={overview}
          rules={naming.value}
          mapped={environments.value}
          limitsShown={limitsShown}
          onAddRule={(rule) => naming.set([...naming.value, rule])}
          onAddMappings={(mappings) => environments.set([...environments.value, ...mappings])}
        />
        <div className="grid gap-[12px] xl:grid-cols-2">
          <ThresholdsCard saved={saved} {...shared} onSave={(thresholds, done) => persist(thresholds, done)} />
          <CatalogRulesCard draft={catalogRules} {...shared} onSave={() => persist({ metadataRules: catalogRules.value }, catalogRules.reset)} />
          <NamingRulesCard draft={naming} {...shared} onSave={() => persist({ namingRules: naming.value }, naming.reset)} />
          <EnvironmentMapCard draft={environments} {...shared} onSave={() => persist({ environmentMap: environments.value }, environments.reset)} />
        </div>
        <AliasesCard draft={aliases} services={services} {...shared} onSave={() => persist({ aliases: aliases.value }, aliases.reset)} />
        {limitsShown && <LimitsCard />}
        {hasPermission('sys.telemetry.read', product) && <SamplingCard />}
      </ApmQueryState>
    </ApmPage>
  );
}
