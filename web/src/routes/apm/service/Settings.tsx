import { BookOpen, RadioTower } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ProfileValue, ServiceDefinition, ServiceSummary, TransactionSummary } from '@/api/apm';

import { KeyTransactionsCard } from './KeyTransactionsCard';
import { entryKindsOf, languageName } from './serviceFacts';
import type { ServiceAddress } from './useService';
import { DefinitionForm } from '../catalog/DefinitionDrawer';
import { ApmCard } from '../frame/Page';

/**
 * A service's settings: its catalog entry, which people keep (team, owner,
 * tier, lifecycle, repository, tags, Apdex threshold, what it does, whether
 * it is archived) and its key transactions, beside what its instances
 * report within the range. Where both say something, the page heads with
 * what is reported; the catalog fills what is not.
 */
export function ServiceSettings({
  address,
  summary,
  definition,
  transactions,
}: {
  address: ServiceAddress;
  summary: ServiceSummary | undefined;
  definition: ServiceDefinition | null;
  transactions: readonly TransactionSummary[];
}) {
  const { t } = useTranslation('apm');

  return (
    <div data-apm-service-settings="" className="grid min-w-0 items-start gap-[12px] xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
      <ApmCard icon={BookOpen} tone="text-[var(--chart-1)]" title={t('settings.catalog')} description={t('settings.catalog_description')} testId="service-catalog-entry">
        <DefinitionForm
          key={`${address.namespace}/${address.name}/${definition?.updatedAt ?? 'new'}`}
          embedded
          target={{ namespace: address.namespace, name: address.name, definition }}
          onDone={() => undefined}
        />
      </ApmCard>
      <div className="grid min-w-0 gap-[12px]">
        <KeyTransactionsCard address={address} transactions={transactions} />
        <ReportedCard address={address} summary={summary} />
      </div>
    </div>
  );
}

function ReportedCard({ address, summary }: { address: ServiceAddress; summary: ServiceSummary | undefined }) {
  const { t } = useTranslation('apm');
  const profile = summary?.profile;
  const sdk = [summary?.instrumentation.telemetrySdkName, summary?.instrumentation.telemetrySdkVersion].filter(Boolean).join(' ');
  const rows: Array<[string, string]> = [
    [t('service.facts.system'), address.namespace],
    [t('service.facts.environment'), address.environment],
    [t('service.facts.type'), entryKindsOf(profile).map((kind) => t(`service.facts.kinds.${kind}`)).join(' · ')],
    [t('settings.reported.instances'), profile ? String(profile.instanceCount) : ''],
    [t('settings.reported.hosts'), profile?.hostCount ? String(profile.hostCount) : ''],
    [t('service.facts.version'), listed(profile?.versions)],
    [t('settings.reported.language'), listed(profile?.languages, languageName) || (summary?.instrumentation.runtimeLanguage ? languageName(summary.instrumentation.runtimeLanguage) : '')],
    [t('settings.reported.runtime'), listed(profile?.runtimes)],
    [t('settings.reported.framework'), listed(profile?.frameworks)],
    [t('settings.reported.sdk'), sdk],
    [t('service.facts.region'), listed(profile?.regions)],
    [t('settings.reported.cluster'), listed(profile?.clusters)],
    [t('settings.reported.k8s_namespace'), listed(profile?.k8sNamespaces)],
    [t('settings.reported.workload'), listed(profile?.workloads)],
    [t('service.facts.repository'), listed(profile?.repositories)],
    [t('settings.reported.commit'), listed(profile?.commits, (commit) => commit.slice(0, 12))],
  ];

  return (
    <ApmCard icon={RadioTower} tone="text-[var(--chart-2)]" title={t('settings.reported.title')} description={t('settings.reported.description')} testId="service-reported">
      <dl className="divide-y divide-bd-0 font-sans text-xs">
        {rows.map(([label, value]) => (
          <div key={label} data-apm-reported={label} className="grid min-h-8 grid-cols-[136px_minmax(0,1fr)] items-center gap-3 px-3 py-1.5">
            <dt className="truncate text-tx-2">{label}</dt>
            <dd className={value ? 'min-w-0 break-words text-tx-0' : 'text-tx-3'}>{value || t('settings.reported.none')}</dd>
          </div>
        ))}
      </dl>
    </ApmCard>
  );
}

/** A ranked list as words, each with how many instances share it when more than one value does. */
function listed(values: readonly ProfileValue[] | undefined, name: (value: string) => string = (value) => value): string {
  if (!values?.length) return '';
  if (values.length === 1) return name(values[0]!.value);
  return values.map((value) => `${name(value.value)} (${value.instances})`).join(', ');
}
