import { useQuery } from '@tanstack/react-query';
import { ChartLine, Flame, type LucideIcon, MonitorSmartphone, ScrollText } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ServiceSummary } from '@/api/apm';
import { apmCatalogApi, apmCatalogQueryKeys, type ServiceCoverage } from '@/api/apmCatalog';
import { cn } from '@/shell/lib/cn';

import { signalHref } from './links';
import type { ServiceAddress } from './useService';

/** How often coverage is looked at again: the backend keeps its looks as long. */
const COVERAGE_SLOT_MICROS = 300_000_000;

const SIGNALS: ReadonlyArray<{ key: 'logs' | 'metrics' | 'profiles'; icon: LucideIcon; tone: string; setup: string }> = [
  { key: 'logs', icon: ScrollText, tone: 'text-[var(--blue-ink)]', setup: '/datasource/applications/opentelemetry?signal=logs' },
  { key: 'metrics', icon: ChartLine, tone: 'text-[var(--green-ink)]', setup: '/datasource/applications/opentelemetry?signal=metrics' },
  { key: 'profiles', icon: Flame, tone: 'text-[var(--orange-ink)]', setup: '/datasource/recommended/continuous-profiling' },
];

/** The coverage of the service in the day before the end of the range in view. */
export function useServiceCoverage(address: ServiceAddress) {
  const to = address.range.to.getTime() * 1_000;
  const slot = Math.floor(to / COVERAGE_SLOT_MICROS);
  const service = { namespace: address.namespace, name: address.name, environment: address.environment };

  return useQuery({
    queryKey: apmCatalogQueryKeys.coverage(address.orgId, service, slot),
    // The same look for the whole slot, so the backend answers from what it kept.
    queryFn: () => apmCatalogApi.coverage(service, (slot + 1) * COVERAGE_SLOT_MICROS - 1),
    enabled: Boolean(address.orgId && address.name && address.environment),
    staleTime: COVERAGE_SLOT_MICROS / 1_000,
    retry: false,
  });
}

/** The RUM page of a session the service served, over the day it was found in. */
export function rumSessionHref(session: string, coverage: Pick<ServiceCoverage, 'from' | 'to'>): string {
  const from = new Date(coverage.from / 1_000).toISOString();
  const to = new Date(coverage.to / 1_000).toISOString();
  return `/rum/sessions/view/${encodeURIComponent(session)}?${new URLSearchParams({ from, to, time: `${from}..${to}` })}`;
}

/**
 * The service's other signals at the end of its navigation card: each one
 * that carries the service lit in its colour and opening there (logs on the
 * stream they were found in), each that does not dimmed and opening the
 * page that connects it. RUM shows only where a browser session was found
 * among the service's sample traces.
 */
export function RelatedSignals({ address, summary }: { address: ServiceAddress; summary: ServiceSummary }) {
  const { t } = useTranslation('apm');
  const coverage = useServiceCoverage(address);
  const data = coverage.data;
  const session = data?.rum.state === 'reported' ? data.rum.found?.[0] : undefined;

  return (
    <span data-apm-related-signals="" className="ml-auto flex items-center gap-3 font-sans text-xs">
      {SIGNALS.map(({ key, icon: Icon, tone, setup }) => {
        const signal = data?.[key];
        const found = signal?.found ?? [];
        const label = t(`service.related.${key}`);

        if (signal?.state === 'missing') {
          return (
            <Link
              key={key}
              to={setup}
              data-apm-related={key}
              data-coverage="missing"
              title={t(`service.coverage.missing.${key}`)}
              className="inline-flex items-center gap-1 text-tx-3 hover:text-tx-1 focus-visible:text-tx-1 focus-visible:outline-none"
            >
              <Icon aria-hidden className="h-3.5 w-3.5 text-tx-4" />
              {label}
            </Link>
          );
        }

        const reported = signal?.state === 'reported';
        return (
          <Link
            key={key}
            to={signalHref(key, summary.traces, key === 'logs' && found[0] ? { stream: found[0] } : undefined)}
            data-apm-related={key}
            data-coverage={signal?.state ?? 'pending'}
            title={reported ? t(`service.coverage.reported.${key}`, { found: found.join(', ') }) : undefined}
            className={cn(
              'inline-flex items-center gap-1 font-strong hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none',
              reported ? 'text-tx-1' : 'text-tx-2',
            )}
          >
            <Icon aria-hidden className={cn('h-3.5 w-3.5', reported ? tone : 'text-tx-3')} />
            {label}
          </Link>
        );
      })}
      {session && data && (
        <Link
          to={rumSessionHref(session, data)}
          data-apm-related="rum"
          data-coverage="reported"
          title={t('service.coverage.reported.rum')}
          className="inline-flex items-center gap-1 font-strong text-tx-1 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none"
        >
          <MonitorSmartphone aria-hidden className="h-3.5 w-3.5 text-[var(--purple-ink)]" />
          {t('service.related.rum')}
        </Link>
      )}
    </span>
  );
}
