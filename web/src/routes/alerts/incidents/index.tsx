import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BellOff, Check, ExternalLink, Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as alertsApi from '@/api/alerts';
import * as alertSourcesApi from '@/api/alertSources';
import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { restrictActionAccess, useActionAccess } from '@/product/actionAccess';
import { ProductState, type ProductStateProps } from '@/product/states';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { FormSelect } from '@/shell/FormDrawer';
import { IncidentDetailDrawer } from '@/shell/incident/DetailDrawer';
import { prioritiesOf, priorityLabel, PriorityText, useImportance } from '@/shell/incident/Importance';
import { useTeams } from '@/shell/incident/Routing';
import { IncidentSilenceDialog } from '@/shell/incident/SilenceDialog';
import { StatusWord, type StatusTone } from '@/shell/StatusWord';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import { useAuthStore } from '@/stores/auth';
import { formatRelativeMicros } from '@/time/relative';
import type { IncidentStatus, Priority } from '@/types/alerting';
import { SeverityRail } from '@/viz/SeverityRail';

import { formatDurationSecs, isActiveIncident } from '../alertRuleModel';
import { AlertsPage } from '../Frame';
import { alertFlatTableClassName } from '../Surfaces';
import { AlertsActivation } from './Activation';
import { IncidentBulkBar } from './BulkBar';
import { IncidentCreateDrawer } from './CreateDrawer';
import {
  byUrgency,
  incidentDurationSecs,
  incidentRunbook,
  incidentService,
  isResolvedIncident,
} from './model';
import { IncidentStanding } from './Standing';
import { IncidentSummaryCell } from './SummaryCell';

type IncidentView = 'active' | 'unacknowledged' | 'my_teams' | 'inhibited';
/** A priority, those without one (`none`), or every incident (`''`). */
type PriorityFilter = Priority | 'none' | '';

const DAY_MICROS = 24 * 60 * 60 * 1_000_000;
/** How many recovered incidents the tab recalls when nothing is firing; History has them all. */
const RECENT_COUNT = 5;

const INCIDENT_TONE: Record<IncidentStatus, StatusTone> = {
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
  const acknowledgeAccess = useActionAccess({ permission: 'alerts.incidents.acknowledge' });
  const silenceAccess = useActionAccess({ permission: 'alerts.incidents.silence' });
  const users = useUsers();
  const currentUserId = useAuthStore((state) => state.ctx?.userId ?? '');
  const teamsQuery = useTeams();
  const [view, setView] = React.useState<IncidentView>('active');
  const [priority, setPriority] = React.useState<PriorityFilter>('');
  const [viewingIncidentId, setViewingIncidentId] = React.useState<string | null>(null);
  const [silencingIncident, setSilencingIncident] = React.useState<{ id: string; name: string } | null>(null);
  const [selected, setSelected] = React.useState<string[]>([]);
  const [creating, setCreating] = React.useState(false);

  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'incidents', 'all'],
    queryFn: () => incidentsApi.list({ scope: 'all', windowSecs: 7 * 24 * 60 * 60 }),
    refetchInterval: 30_000,
  });
  const rulesQuery = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: () => alertsApi.list(),
    refetchInterval: 30_000,
  });
  // The names of the alert sources whose incidents are listed: a nicety, so an answer that is not
  // a list names nothing rather than breaking the page.
  const sourcesQuery = useQuery({
    queryKey: ['alert-sources'],
    queryFn: () => alertSourcesApi.list(),
    staleTime: 30_000,
    retry: false,
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
    .sort((left, right) => (right.resolvedAt ?? 0) - (left.resolvedAt ?? 0));
  const recoveredToday = recovered.filter(
    (incident) => incident.resolvedAt !== undefined && nowMicros - incident.resolvedAt <= DAY_MICROS,
  );
  const ruleById = new Map(rules.map((rule) => [rule.id, rule]));
  const sourceById = new Map(
    (Array.isArray(sourcesQuery.data) ? sourcesQuery.data : []).map((source) => [source.id, source]),
  );
  const teams = Array.isArray(teamsQuery.data) ? teamsQuery.data : [];
  const teamNames = new Map(teams.map((team) => [team.id, team.name]));
  // The teams the reader is in: their view is offered only to someone in one.
  const myTeamIds = new Set(teams.filter((team) => team.memberIds.includes(currentUserId)).map((team) => team.id));
  const mine = active.filter((incident) => incident.teamId && myTeamIds.has(incident.teamId));
  const inhibited = active.filter((incident) => incident.inhibition);
  const shownView: IncidentView =
    (view === 'my_teams' && myTeamIds.size === 0) || (view === 'inhibited' && inhibited.length === 0) ? 'active' : view;
  const viewRows =
    shownView === 'active' ? active : shownView === 'unacknowledged' ? waiting : shownView === 'inhibited' ? inhibited : mine;
  const rows =
    priority === ''
      ? viewRows
      : viewRows.filter((incident) => (priority === 'none' ? !incident.priority : incident.priority === priority));
  // Only incidents still listed stay ticked: one that recovered or moved out of the view drops out.
  const picked = rows.filter((incident) => selected.includes(incident.id));
  const selectable = acknowledgeAccess.allowed;
  const toggle = (id: string, on: boolean) =>
    setSelected((current) => (on ? [...current.filter((other) => other !== id), id] : current.filter((other) => other !== id)));

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
  // What no rule caught is opened by hand: a customer's report, something someone saw.
  const newIncident = (
    <ChromeButton
      variant="primary"
      disabled={acknowledgeAccess.disabled}
      disabledReason={acknowledgeAccess.reason}
      onClick={() => setCreating(true)}
      data-testid="incident-create"
    >
      <Plus className="h-4 w-4" />
      {t('create_incident.open')}
    </ChromeButton>
  );

  const body = () => {
    if (active.length > 0) {
      return rows.length === 0 ? (
        // Only a narrower view can be empty while something is firing.
        <ProductState
          variant="empty"
          title={
            viewRows.length > 0
              ? t('center.incidents.empty.priority.title')
              : t(`center.incidents.empty.${shownView === 'my_teams' ? 'my_teams' : 'unacknowledged'}.title`)
          }
          description={
            viewRows.length > 0
              ? t('center.incidents.empty.priority.description')
              : t(`center.incidents.empty.${shownView === 'my_teams' ? 'my_teams' : 'unacknowledged'}.description`)
          }
          className="rounded-none border-0 bg-transparent shadow-none"
        />
      ) : (
        <>
        {picked.length > 0 && <IncidentBulkBar incidents={picked} onClear={() => setSelected([])} />}
        <DataTable
          className={alertFlatTableClassName}
          rows={rows}
          rowKey={(incident) => incident.id}
          onRowClick={(incident) => setViewingIncidentId(incident.id)}
          columns={[
            ...(selectable
              ? [
                  {
                    key: 'select',
                    width: 40,
                    header: (
                      <Checkbox
                        aria-label={t('incident_actions.select_all')}
                        checked={picked.length === rows.length ? true : picked.length > 0 ? 'indeterminate' : false}
                        onCheckedChange={(next) => setSelected(next === true ? rows.map((incident) => incident.id) : [])}
                      />
                    ),
                    cell: (incident: (typeof rows)[number]) => (
                      <Checkbox
                        aria-label={t('incident_actions.select', { name: incident.summary })}
                        checked={selected.includes(incident.id)}
                        onCheckedChange={(next) => toggle(incident.id, next === true)}
                        data-incident-select
                      />
                    ),
                  },
                ]
              : []),
            {
              key: 'incident',
              header: t('center.incidents.columns.incident'),
              cell: (incident) => (
                <IncidentSummaryCell
                  incident={incident}
                  extra={ruleById.get(incident.ruleId)?.name ?? sourceById.get(incident.ruleId)?.name}
                />
              ),
            },
            {
              key: 'priority',
              header: t('center.incidents.columns.priority'),
              width: 80,
              cell: (incident) => <PriorityText priority={incident.priority} />,
            },
            {
              key: 'status',
              header: t('center.incidents.columns.status'),
              width: 130,
              cell: (incident) => (
                <div className="min-w-0 py-1">
                  <StatusWord tone={INCIDENT_TONE[incident.status]} strong={incident.status === 'open'}>
                    {t(`center.incidents.status.${incident.status}`)}
                  </StatusWord>
                  {incident.snoozedUntil && (
                    <span className="mt-0.5 block truncate text-xs text-tx-2" data-incident-snoozed>
                      {t('incident_actions.snoozed_until', { time: formatMicrosActive(incident.snoozedUntil) })}
                    </span>
                  )}
                </div>
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
                const owner = incident.acknowledgedBy ?? incident.assignees?.[0];
                const team = incident.teamId ? teamNames.get(incident.teamId) : undefined;

                // "Waiting" already says nobody has it; the column names a person or stays empty,
                // with the team routing gave the incident under it.
                return (
                  <div className="min-w-0 py-1">
                    {owner ? (
                      <span className="block truncate text-tx-1">{users.nameOf(owner)}</span>
                    ) : (
                      <span className="block text-tx-3">—</span>
                    )}
                    {team && (
                      <span className="mt-0.5 block truncate text-xs text-tx-2" data-incident-team-name>
                        {team}
                      </span>
                    )}
                  </div>
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
                const runbook = incidentRunbook(incident, ruleById.get(incident.ruleId)?.annotations);
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
        </>
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
                <SeverityRail severity={incident.severity} className="w-[2px] shrink-0 rounded-full" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-sans text-sm font-strong text-tx-0">{incident.summary}</span>
                  <span className="mt-0.5 block truncate text-xs text-tx-2">
                    {[
                      t(`severity.${incident.severity}`),
                      incidentService(incident),
                      t('center.incidents.recent.recovered', {
                        when: formatRelativeMicros(incident.resolvedAt, i18n.language, nowMicros),
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
        <AlertsPage contentSurface={false} action={newIncident}>
          <AlertsActivation />
        </AlertsPage>
      ) : (
        <AlertsPage
          action={newIncident}
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
                  <>
                    <ViewTabs<IncidentView>
                      label={t('center.incidents.tabs_label')}
                      value={shownView}
                      onValueChange={setView}
                      dataAttributes={{ 'data-alert-filter-tabs': '' }}
                      // The statement above has counted the first two; the views only choose among what is firing.
                      items={[
                        { key: 'active', label: t('center.incidents.tabs.active') },
                        { key: 'unacknowledged', label: t('center.incidents.tabs.unacknowledged') },
                        ...(myTeamIds.size > 0 ? [{ key: 'my_teams' as const, label: t('center.incidents.tabs.my_teams') }] : []),
                        // Offered only while a rule holds something back.
                        ...(inhibited.length > 0
                          ? [{ key: 'inhibited' as const, label: t('center.incidents.tabs.inhibited'), count: inhibited.length }]
                          : []),
                      ]}
                    />
                    <PrioritySelect value={priority} onChange={setPriority} className="ml-auto w-full sm:w-[140px]" />
                  </>
                ),
              }
            : {})}
        >
          {hasBody ? body() : null}
        </AlertsPage>
      )}
      <IncidentDetailDrawer incidentId={viewingIncidentId} onClose={() => setViewingIncidentId(null)} />
      <IncidentCreateDrawer open={creating} onClose={() => setCreating(false)} />
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

/** Narrows a list of incidents to one priority, or to those without one. */
export function PrioritySelect({
  value,
  onChange,
  className,
}: {
  value: PriorityFilter;
  onChange: (value: PriorityFilter) => void;
  className: string;
}) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const priorities = prioritiesOf(levels);

  return (
    <FormSelect
      ariaLabel={t('importance.filter_label')}
      className={className}
      value={value}
      onChange={(next) => onChange(next as PriorityFilter)}
      options={[
        { value: '', label: t('importance.all_priorities') },
        ...priorities.map((item) => ({ value: item, label: priorityLabel(levels, item) })),
        { value: 'none', label: t('importance.without_priority') },
      ]}
    />
  );
}
