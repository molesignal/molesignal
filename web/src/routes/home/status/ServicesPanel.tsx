import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ServiceSummary } from '@/api/apm';
import { formatDuration, formatThroughput } from '@/routes/apm/format';
import { Sparkline } from '@/shell/readout/Sparkline';
import { StatusWord } from '@/shell/StatusWord';

import { formatShare, rankServices, SERVICE_TONE } from './model';
import { Panel, PanelNote } from './Panel';
import type { HomeStatus } from './useHomeStatus';

const SERVICE_ROWS = 8;

function servicePath(service: ServiceSummary): string {
  const query = new URLSearchParams({ namespace: service.service.namespace, environment: service.service.environment });
  return `/apm/services/${encodeURIComponent(service.service.name)}?${query.toString()}`;
}

/** A figure past the line that makes the service unhealthy takes the service's ink. */
function reasonTone(service: ServiceSummary, metric: 'error_rate' | 'p95'): string {
  const reason = service.healthReasons?.find((candidate) => candidate.metric === metric);
  if (!reason) return 'text-tx-1';
  return reason.level === 'critical' ? 'text-[var(--red-ink)]' : 'text-[var(--yellow-ink)]';
}

/** The services to look at first: the unhealthy ones, then the busiest, each with its figures and its traffic. */
export function ServicesPanel({ status, className }: { status: HomeStatus; className?: string }) {
  const { t } = useTranslation('onboarding');
  const overview = status.apm.data;
  const services = Array.isArray(overview?.services) ? overview.services : [];
  const range = overview?.meta?.range;
  const windowSecs = range ? Math.max(1, (range.to - range.from) / 1_000_000) : 1;
  const rows = rankServices(services, SERVICE_ROWS);
  const total = overview?.serviceCount ?? services.length;

  return (
    <Panel
      title={t('home.services.title')}
      count={overview ? total : undefined}
      action={{ label: t('home.view_all'), to: '/apm/services' }}
      className={className}
      testId="home-services"
    >
      {status.apm.isError ? (
        <PanelNote>{t('home.unavailable')}</PanelNote>
      ) : !overview ? (
        <PanelNote>{t('home.loading')}</PanelNote>
      ) : rows.length === 0 ? (
        <PanelNote testId="home-services-empty">
          {t('home.services.empty')}{' '}
          <Link to="/datasource" className="font-strong text-[var(--blue-ink)] hover:underline">
            {t('home.services.connect')}
          </Link>
        </PanelNote>
      ) : (
        <table className="w-full table-fixed border-collapse font-sans text-xs">
          <thead>
            <tr className="h-[32px] text-left text-tx-2 [&>th]:font-strong">
              <th className="pl-[12px]">{t('home.services.service')}</th>
              <th className="w-[72px]">{t('home.services.health')}</th>
              <th className="w-[26%]">{t('home.services.throughput')}</th>
              <th className="w-[80px] text-right">{t('home.services.error_rate')}</th>
              <th className="w-[88px] pr-[12px] text-right">P95</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((service) => (
              <tr
                key={`${service.service.namespace}/${service.service.name}/${service.service.environment}`}
                data-home-service={service.service.name}
                className="h-[40px] border-t border-[var(--row-rule)] hover:bg-[var(--bg-hover)]"
              >
                <td className="truncate pl-[12px]">
                  <Link to={servicePath(service)} className="font-strong text-tx-0 hover:underline">
                    {service.service.name}
                  </Link>
                  {service.service.environment && <span className="ml-1.5 text-tx-3">{service.service.environment}</span>}
                </td>
                <td>
                  <StatusWord tone={SERVICE_TONE[service.health]}>{t(`home.service_health.${service.health}`)}</StatusWord>
                </td>
                <td>
                  <span className="flex items-center gap-2 pr-3">
                    <Sparkline
                      values={(service.sparkline ?? []).map((point) => point.requestCount)}
                      variant="bars"
                      className="h-[20px] min-w-0 flex-1 text-[var(--chart-1)]"
                    />
                    <span className="w-[72px] shrink-0 text-right tabular-nums text-tx-1">
                      {service.red.requestCount > 0 ? formatThroughput(service.red.requestCount / windowSecs) : '—'}
                    </span>
                  </span>
                </td>
                <td className={`text-right tabular-nums ${reasonTone(service, 'error_rate')}`}>
                  {service.red.requestCount > 0 ? formatShare(service.red.errorRate) : '—'}
                </td>
                <td className={`pr-[12px] text-right tabular-nums ${reasonTone(service, 'p95')}`}>
                  {service.red.requestCount > 0 ? formatDuration(service.red.p95Time) : '—'}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Panel>
  );
}
