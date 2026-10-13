import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BellOff, Check, ExternalLink } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as alertsApi from '@/api/alerts';
import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { restrictActionAccess, useActionAccess } from '@/product/actionAccess';
import { ProductState, type ProductStateProps } from '@/product/states';
import { ChromeButton, Dot, IconButton, Pill, type PillTone } from '@/shell/chrome';
import { IncidentDetailDrawer } from '@/shell/incident/DetailDrawer';
import { IncidentSilenceDialog } from '@/shell/incident/SilenceDialog';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import { formatRelativeMicros } from '@/time/relative';
import type { IncidentStatus } from '@/types/alerting';
import { SeverityRail } from '@/viz/SeverityRail';

import { formatDurationSecs, isActiveIncident } from '../alertRuleModel';
import { AlertsPage } from '../Frame';
import { alertFlatTableClassName } from '../Surfaces';
import { AlertsActivation } from './Activation';
import {
  byUrgency,
  incidentDurationSecs,
  incidentRunbook,
  incidentService,
  isResolvedIncident,
} from './model';
import { IncidentStanding } from './Standing';
import { IncidentSummaryCell } from './SummaryCell';

type IncidentView = 'active' | 'unacknowledged';

const DAY_MICROS = 24 * 60 * 60 * 1_000_000;
/** How many recovered incidents the tab recalls when nothing is firing; History has them all. */
const RECENT_COUNT = 5;

const INCIDENT_TONE: Record<IncidentStatus, PillTone> = {
  open: 'red',
  acknowledged: 'yellow',
  resolved: 'green',
  closed: 'dim',
};

/**
 * What is firing now, and who is on it.
 *
 * The tab is about the present. It opens with one statement, lists the
 * incidents that statement counts, the worst first, and offers on each row
 * only what moves it forward. What has recovered is the History tab's; when
 * nothing is firing, the last few recoveries are recalled here so the page
 * still says how things have been.
 */
export function AlertIncidentsPage() {
  const { t, i18n } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const acknowledgeAccess = useActionAccess({ permission: 'alerts.acknowledge' });
  const silenceAccess = useActionAccess({ permission: 'alerts.silence' });
  const users = useUsers();
  const [view, setView] = React.useState<IncidentView>('active');
  const [viewingIncidentId, setViewingIncidentId] = React.useState<string | null>(null);
  const [silencingIncident, setSilencingIncident] = React.useState<{ id: string; name: string } | null>(null);

  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'incidents', 'all'],
    queryFn: () => incidentsApi.list({ scope: 'all', window_secs: 7 * 24 * 60 * 60 }),
    refetchInterval: 30_000,
  });
  const rulesQuery = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: () => alertsApi.list(),
    refetchInterval: 30_000,
  });
  const acknowledge = useMutation({
    mutationFn: (id: string) => incidentsApi.ack(id),
    onSuccess: async () => {
      toast.success(t('drawer.actions.ack_success'));
      await queryClient.invalidateQueries({ queryKey: ['alerts', 'incidents'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const nowMicros = Date.now() * 1000;
  const incidents = incidentsQuery.data ?? [];
  const rules = rulesQuery.data ?? [];
  const active = incidents.filter(isActiveIncident).sort(byUrgency);
  const waiting = active.filter((incident) => incident.status === 'open');
  const recovered = incidents
    .filter(isResolvedIncident)
    .sort((left, right) => (right.resolved_at ?? 0) - (left.resolved_at ?? 0));
  const recoveredToday = recovered.filter(
    (incident) => incident.resolved_at !== undefined && nowMicros - incident.resolved_at <= DAY_MICROS,
  );
  const ruleById = new Map(rules.map((rule) => [rule.id, rule]));
  const rows = view === 'active' ? active : waiting;

  const state: ProductStateProps | null =
    incidentsQuery.isLoading || rulesQuery.isLoading
      ? { variant: 'loading' }
      : incidentsQuery.isError || rulesQuery.isError
        ? {
            variant: 'error',
            title: t('center.incidents.load_error'),
            error: incidentsQuery.error ?? rulesQuery.error,
            action: (
              <ChromeButton
                onClick={() => {
                  void incidentsQuery.refetch();
                  void rulesQuery.refetch();
                }}
              >
                {t('center.retry')}
              </ChromeButton>
            ),
          }
        : null;
  // Nothing can fire until something is watched: the tab says how to begin instead.
  const firstRun = !state && rules.length === 0 && incidents.length === 0;

  const body = () => {
    if (active.length > 0) {
      return rows.length === 0 ? (
        // Only the "waiting" view can be empty while something is firing.
        <ProductState
          variant="empty"
          title={t('center.incidents.empty.unacknowledged.title')}
          description={t('center.incidents.empty.unacknowledged.description')}
          className="rounded-none border-0 bg-transparent shadow-none"
        />
      ) : (
        <DataTable
          className={alertFlatTableClassName}
          rows={rows}
          rowKey={(incident) => incident.id}
          onRowClick={(incident) => setViewingIncidentId(incident.id)}
          columns={[
            {
              key: 'incident',
              header: t('center.incidents.columns.incident'),
              cell: (incident) => (
                <IncidentSummaryCell incident={incident} extra={ruleById.get(incident.rule_id)?.name} />
              ),
            },
            {
              key: 'status',
              header: t('center.incidents.columns.status'),
              width: 130,
              cell: (incident) => (
                <Pill tone={INCIDENT_TONE[incident.status]}>
                  {incident.status === 'open' && <Dot tone="red" />}
                  {t(`center.incidents.status.${incident.status}`)}
                </Pill>
              ),
            },
            {
              key: 'duration',
              header: t('center.incidents.columns.duration'),
              width: 110,
              cell: (incident) => (
                <span className="tabular-nums text-tx-1">
                  {formatDurationSecs(incidentDurationSecs(incident, nowMicros))}
                </span>
              ),
            },
            {
              key: 'owner',
              header: t('center.incidents.columns.owner'),
              width: 180,
              cell: (incident) => {
                const owner = incident.acknowledged_by ?? incident.assignees?.[0];

                // "Waiting" already says nobody has it; the column names a person or stays empty.
                return owner ? (
                  <span className="block truncate text-tx-1">{users.nameOf(owner)}</span>
                ) : (
                  <span className="text-tx-3">—</span>
                );
              },
            },
            {
              key: 'actions',
              header: t('center.incidents.columns.actions'),
              width: 210,
              className: 'overflow-visible text-right',
              headerClassName: 'text-right',
              cell: (incident) => {
                const runbook = incidentRunbook(incident, ruleById.get(incident.rule_id)?.annotations);
                const acknowledgeHere = restrictActionAccess(
                  acknowledgeAccess,
                  incident.status === 'open',
                  t('drawer.actions.ack_unavailable', { defaultValue: 'Only open incidents can be acknowledged.' }),
                );

                return (
                  <div
                    className="flex items-center justify-end gap-[2px]"
                    onClick={(event) => event.stopPropagation()}
                  >
                    {/* The step that moves an incident forward is a button; the rest are at hand as icons. */}
                    {incident.status === 'open' && (
                      <ChromeButton
                        size="sm"
                        className="mr-[4px]"
                        disabled={acknowledgeHere.disabled || acknowledge.isPending}
                        disabledReason={acknowledgeHere.reason}
                        onClick={() => acknowledge.mutate(incident.id)}
                      >
                        <Check className="h-3.5 w-3.5" />
                        {t('center.incidents.actions.ack')}
                      </ChromeButton>
                    )}
                    {runbook && (
                      <IconButton
                        aria-label={t('center.incidents.actions.runbook')}
                        title={t('center.incidents.actions.runbook')}
                        onClick={() => window.open(runbook, '_blank', 'noopener,noreferrer')}
                      >
                        <ExternalLink className="h-3.5 w-3.5" />
                      </IconButton>
                    )}
                    <IconButton
                      aria-label={t('actions.silence_incident', { name: incident.summary })}
                      title={t('center.incidents.actions.silence')}
                      disabled={silenceAccess.disabled}
                      disabledReason={silenceAccess.reason}
                      onClick={() => setSilencingIncident({ id: incident.id, name: incident.summary })}
                    >
                      <BellOff className="h-3.5 w-3.5" />
                    </IconButton>
                  </div>
                );
              },
            },
          ]}
        />
      );
    }

    return (
      <section aria-labelledby="recently-recovered-heading">
        <h2
          id="recently-recovered-heading"
          className="flex min-h-12 items-center px-4 font-sans text-sm font-display-strong text-tx-0"
        >
          {t('center.incidents.recent.title')}
        </h2>
        <ul className="divide-y divide-bd-0 border-t border-bd-0">
          {recovered.slice(0, RECENT_COUNT).map((incident) => (
            <li key={incident.id}>
              <button
                type="button"
                onClick={() => setViewingIncidentId(incident.id)}
                className="flex w-full min-w-0 items-stretch gap-[12px] px-4 py-[10px] text-left outline-none transition-colors hover:bg-bg-2 focus-visible:bg-bg-2"
              >
                <SeverityRail severity={incident.severity} className="w-[3px] shrink-0 rounded-full" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-sans text-sm font-strong text-tx-0">{incident.summary}</span>
                  <span className="mt-0.5 block truncate text-xs text-tx-2">
                    {[
                      t(`severity.${incident.severity}`),
                      incidentService(incident),
                      t('center.incidents.recent.recovered', {
                        when: formatRelativeMicros(incident.resolved_at, i18n.language, nowMicros),
                      }),
                      t('center.incidents.recent.lasted', {
                        duration: formatDurationSecs(incidentDurationSecs(incident, nowMicros)),
                      }),
                    ]
                      .filter(Boolean)
                      .join(' · ')}
                  </span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      </section>
    );
  };

  // When nothing is firing and nothing recovered lately, the statement is the whole page.
  const hasBody = active.length > 0 || recovered.length > 0;

  return (
    <>
      {state ? (
        <AlertsPage state={state} />
      ) : firstRun ? (
        <AlertsPage contentSurface={false}>
          <AlertsActivation />
        </AlertsPage>
      ) : (
        <AlertsPage
          lead={
            <IncidentStanding
              active={active}
              waiting={waiting}
              recovered={recoveredToday}
              ruleCount={rules.filter((rule) => rule.enabled).length}
              nowMicros={nowMicros}
            />
          }
          contentSurface={hasBody}
          {...(active.length > 0
            ? {
                listHead: (
                  <ViewTabs<IncidentView>
                    label={t('center.incidents.tabs_label')}
                    value={view}
                    onValueChange={setView}
                    dataAttributes={{ 'data-alert-filter-tabs': '' }}
                    // The statement above has counted both; the views only choose between them.
                    items={[
                      { key: 'active', label: t('center.incidents.tabs.active') },
                      { key: 'unacknowledged', label: t('center.incidents.tabs.unacknowledged') },
                    ]}
                  />
                ),
              }
            : {})}
        >
          {hasBody ? body() : null}
        </AlertsPage>
      )}
      <IncidentDetailDrawer incidentId={viewingIncidentId} onClose={() => setViewingIncidentId(null)} />
      <IncidentSilenceDialog
        incidentId={silencingIncident?.id ?? null}
        incidentName={silencingIncident?.name}
        open={silencingIncident !== null}
        onOpenChange={(open) => {
          if (!open) setSilencingIncident(null);
        }}
      />
    </>
  );
}
