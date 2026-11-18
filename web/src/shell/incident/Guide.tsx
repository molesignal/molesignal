import { useQuery } from '@tanstack/react-query';
import { ArrowUpRight, BookOpenCheck, LayoutDashboard } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as dashboardsApi from '@/api/dashboards';
import { hasPermission, useProductAccess } from '@/product/access';
import { MarkdownMessage } from '@/routes/agent/markdown';
import { Pill } from '@/shell/chrome';
import type { AlertRule } from '@/types/alerting';

import { guideDashboardLink, guideIsEmpty, guideQueryLink } from './guideModel';
import type { IncidentTimeWindow } from './InvestigationActions';
import { IncidentSection } from './Section';

/**
 * What the rule's investigation guide asks of whoever looks into the incident:
 * its steps, its queries opened over the incident's time range, and its
 * dashboards. Nothing when the rule has no guide.
 */
export function IncidentGuide({
  rule,
  time,
  surface,
}: {
  rule?: AlertRule | undefined;
  time: IncidentTimeWindow;
  surface: boolean;
}) {
  const { t } = useTranslation('alerts');
  const access = useProductAccess();
  const guide = rule?.guide;
  const dashboardIds = guide?.dashboards ?? [];
  // Dashboards are named by their titles where the reader can read them, by their ids otherwise.
  const dashboards = useQuery({
    queryKey: ['dashboards'],
    queryFn: () => dashboardsApi.list(),
    enabled: dashboardIds.length > 0 && hasPermission('dashboards.read', access),
    staleTime: 60_000,
  });

  if (!guide || guideIsEmpty(guide)) return null;

  const steps = (guide.steps ?? '').trim();
  const queries = guide.queries ?? [];
  const titleOf = (id: string) => dashboards.data?.find((dashboard) => dashboard.id === id)?.title ?? id;

  return (
    <IncidentSection
      title={t('drawer.incident_sections.guide')}
      icon={<BookOpenCheck className="h-3.5 w-3.5" />}
      surface={surface}
    >
      <div data-incident-guide className="flex flex-col gap-3">
        {steps && (
          <div className="rounded-md bg-bg-2 px-3 py-2.5">
            <MarkdownMessage
              content={steps}
              className="font-sans text-xs [&_h1]:text-sm [&_h2]:text-sm [&_h3]:text-xs [&_h4]:text-xs"
            />
          </div>
        )}

        {queries.length > 0 && (
          <div className="flex flex-col gap-1.5">
            <span className="font-sans text-xs font-strong text-tx-2">{t('drawer.guide.queries')}</span>
            <ul className="flex flex-col gap-1.5">
              {queries.map((query, index) => (
                <li key={`${index}-${query.name}`} data-guide-query-link>
                  <Link
                    to={guideQueryLink(query, time)}
                    className="group flex min-w-0 flex-col gap-1 rounded-md bg-bg-2 px-3 py-2 transition-colors hover:bg-bg-3 focus-visible:bg-bg-3"
                  >
                    <span className="flex min-w-0 items-center gap-2 font-sans text-xs font-strong text-tx-0">
                      <Pill tone="indigo">{query.language === 'promql' ? 'PromQL' : 'SQL'}</Pill>
                      <span className="min-w-0 truncate">{query.name}</span>
                      <ArrowUpRight
                        className="ml-auto h-3.5 w-3.5 shrink-0 text-tx-3 group-hover:text-indigo-soft"
                        aria-hidden
                      />
                    </span>
                    <code className="truncate font-mono text-xs text-tx-2">{query.statement}</code>
                  </Link>
                </li>
              ))}
            </ul>
          </div>
        )}

        {dashboardIds.length > 0 && (
          <div className="flex flex-col gap-1.5">
            <span className="font-sans text-xs font-strong text-tx-2">{t('drawer.guide.dashboards')}</span>
            <ul className="flex flex-wrap gap-1.5">
              {dashboardIds.map((id) => (
                <li key={id}>
                  <Link
                    to={guideDashboardLink(id)}
                    className="flex min-w-0 items-center gap-1.5 rounded-md bg-bg-2 px-2.5 py-1 font-sans text-xs text-tx-0 transition-colors hover:bg-bg-3 hover:text-indigo-soft focus-visible:bg-bg-3"
                  >
                    <LayoutDashboard className="h-3.5 w-3.5 shrink-0 text-tx-2" aria-hidden />
                    <span className="truncate">{titleOf(id)}</span>
                  </Link>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </IncidentSection>
  );
}
