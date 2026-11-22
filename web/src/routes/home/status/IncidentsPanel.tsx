import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { StatusWord } from '@/shell/StatusWord';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';

import { formatAge } from '../format';
import { SEVERITY_FILL, SEVERITY_TONE } from './model';
import { Panel, PanelNote } from './Panel';
import { HOME_INCIDENTS_QUERY_KEY, type HomeStatus } from './useHomeStatus';

const INCIDENT_ROWS = 6;

/** The incidents going on, the most severe and oldest first, with the way to answer one. */
export function IncidentsPanel({ status, className }: { status: HomeStatus; className?: string }) {
  const { t } = useTranslation('onboarding');
  const qc = useQueryClient();
  const { nameOf } = useUsers();
  const summary = status.summaries.incidents;
  const acknowledgeAccess = useActionAccess({ permission: 'alerts.incidents.acknowledge' });
  const acknowledge = useMutation({
    mutationFn: incidentsApi.ack,
    onSuccess: () => void qc.invalidateQueries({ queryKey: HOME_INCIDENTS_QUERY_KEY }),
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <Panel
      title={t('home.incidents.title')}
      count={summary?.active.length}
      action={{ label: t('home.view_all'), to: '/alerts/incidents' }}
      className={className}
      testId="home-incidents"
    >
      {status.incidents.isError ? (
        <PanelNote>{t('home.unavailable')}</PanelNote>
      ) : !summary ? (
        <PanelNote>{t('home.loading')}</PanelNote>
      ) : summary.active.length === 0 ? (
        <PanelNote testId="home-incidents-empty">{t('home.incidents.empty')}</PanelNote>
      ) : (
        <ul className="min-h-0 flex-1 overflow-y-auto">
          {summary.active.slice(0, INCIDENT_ROWS).map((incident) => {
            const service = incident.affectedServices[0];
            const assignee = incident.assignees[0];
            return (
              <li
                key={incident.id}
                data-home-incident={incident.id}
                className="relative flex min-h-[48px] items-center gap-3 border-t border-[var(--row-rule)] py-[6px] pl-[14px] pr-[12px] first:border-t-0 hover:bg-[var(--bg-hover)]"
              >
                <span
                  aria-hidden="true"
                  className="absolute bottom-[8px] left-0 top-[8px] w-[2px] rounded-r-full"
                  style={{ background: SEVERITY_FILL[incident.severity] }}
                />
                <div className="min-w-0 flex-1">
                  <Link
                    to={`/alerts/incidents/${encodeURIComponent(incident.id)}`}
                    className="block truncate font-sans text-sm text-tx-0 after:absolute after:inset-0"
                  >
                    {incident.summary || incident.id}
                  </Link>
                  <div className="mt-0.5 truncate font-sans text-xs text-tx-3">
                    <StatusWord tone={SEVERITY_TONE[incident.severity]}>{t(`home.severity.${incident.severity}`)}</StatusWord>
                    {service && <> · {service}</>}
                    {' · '}
                    <span className="tabular-nums">{formatAge(incident.createdAt)}</span>
                    {assignee && <> · {nameOf(assignee)}</>}
                  </div>
                </div>
                {incident.status === 'open' && acknowledgeAccess.allowed ? (
                  <ChromeButton
                    size="sm"
                    className="relative z-10"
                    disabled={acknowledge.isPending && acknowledge.variables === incident.id}
                    onClick={() => acknowledge.mutate(incident.id)}
                  >
                    {t('home.incidents.acknowledge')}
                  </ChromeButton>
                ) : (
                  <StatusWord tone={incident.status === 'open' ? 'red' : 'neutral'} className="text-xs">
                    {t(`home.incident_status.${incident.status}`)}
                  </StatusWord>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </Panel>
  );
}
