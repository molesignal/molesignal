import { Globe, Radar } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ApmMeta, EnvironmentShare, InstrumentationQuality } from '@/api/apm';
import { cn } from '@/shell/lib/cn';

import { formatCount, formatRate } from '../format';
import { ApmCard } from '../frame/Page';
import { ApmQualityChip } from '../frame/QualityChip';

const ROW = 'grid h-8 items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs last:border-b-0';

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{children}</p>;
}

/** Where the services run: each environment's services and its share of the requests. */
export function EnvironmentsCard({ environments }: { environments: readonly EnvironmentShare[] }) {
  const { t } = useTranslation('apm');
  const total = environments.reduce((sum, item) => sum + item.requestCount, 0);

  return (
    <ApmCard icon={Globe} tone="text-[var(--chart-2)]" title={t('landing.environments')} testId="overview-environments">
      {environments.length === 0 ? (
        <Empty>{t('landing.no_environments')}</Empty>
      ) : (
        environments.map((item) => {
          const share = total > 0 ? item.requestCount / total : 0;

          return (
            <div key={item.environment} data-apm-environment={item.environment} className={cn(ROW, 'grid-cols-[minmax(0,1fr)_88px_120px_56px]')}>
              <span className="truncate font-strong text-tx-0">{item.environment}</span>
              <span className="text-right tabular-nums text-tx-2">{t('landing.environment_services', { count: item.serviceCount })}</span>
              <span aria-hidden className="relative h-1.5 overflow-hidden rounded-full bg-bg-3">
                <span className="absolute inset-y-0 left-0 rounded-full bg-[var(--chart-1)]" style={{ width: `${Math.max(2, share * 100)}%` }} />
              </span>
              <span className="text-right tabular-nums text-tx-1">{formatRate(share)}</span>
            </div>
          );
        })
      )}
    </ApmCard>
  );
}

/**
 * What the services report about how they are instrumented, and what they
 * leave out that APM reads them by: an environment, a version, an
 * instance. A gap leads to the OpenTelemetry intake guide.
 */
export function InstrumentationCard({ quality, meta }: { quality: InstrumentationQuality; meta: ApmMeta }) {
  const { t } = useTranslation('apm');
  const gaps = [
    ['without_environment', quality.withoutEnvironment, 'deployment.environment.name'],
    ['without_version', quality.withoutVersion, 'service.version'],
    ['without_instance', quality.withoutInstance, 'service.instance.id'],
  ] as const;
  const missing = gaps.filter(([, count]) => count > 0);

  return (
    <ApmCard icon={Radar} tone="text-[var(--blue-ink)]" title={t('landing.instrumentation')} testId="overview-instrumentation" actions={<ApmQualityChip meta={meta} />}>
      {quality.sdks.length === 0 ? (
        <Empty>{t('landing.no_sdks')}</Empty>
      ) : (
        quality.sdks.slice(0, 6).map((sdk) => (
          <div
            key={`${sdk.language ?? ''}/${sdk.sdkName ?? ''}/${sdk.sdkVersion ?? ''}`}
            className={cn(ROW, 'grid-cols-[96px_minmax(0,1fr)_88px]')}
          >
            <span className="truncate font-strong text-tx-0">{sdk.language ?? t('landing.unknown')}</span>
            <span className="truncate text-tx-2">{[sdk.sdkName, sdk.sdkVersion].filter(Boolean).join(' ') || t('landing.unknown')}</span>
            <span className="text-right tabular-nums text-tx-1">{t('landing.environment_services', { count: sdk.serviceCount })}</span>
          </div>
        ))
      )}
      {missing.length > 0 && (
        <div data-apm-instrumentation-gaps="" className="space-y-1 border-t border-bd-0 px-3 py-2 font-sans text-xs text-tx-2">
          {missing.map(([key, count, attribute]) => (
            <p key={key}>
              <span className="font-strong text-[var(--yellow-ink)]">{t('landing.gap', { count, formatted: formatCount(count) })}</span>{' '}
              {t('landing.gap_attribute')} <code className="font-sans text-tx-1">{attribute}</code>
            </p>
          ))}
          <Link
            to="/datasource/applications/opentelemetry?signal=traces"
            className="inline-flex font-strong text-indigo-soft hover:underline focus-visible:underline focus-visible:outline-none"
          >
            {t('landing.gap_action')}
          </Link>
        </div>
      )}
    </ApmCard>
  );
}
