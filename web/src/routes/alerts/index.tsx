import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BellOff,
  Check,
  ChevronDown,
  ExternalLink,
  Eye,
  Pencil,
  Plus,
  Search,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as alertsApi from '@/api/alerts';
import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { restrictActionAccess, useActionAccess } from '@/product/actionAccess';
import { ListPage } from '@/product/templates';
import {
  ChromeButton,
  Dot,
  IconButton,
  Pill,
  type PillTone,
} from '@/shell/chrome';
import { EmptyState } from '@/shell/EmptyState';
import { ErrorState } from '@/shell/ErrorState';
import { IncidentDetailDrawer } from '@/shell/incident/DetailDrawer';
import { IncidentSilenceDialog } from '@/shell/incident/SilenceDialog';
import { LoadingState } from '@/shell/LoadingState';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import type {
  AlertRule,
  AlertRuleKind,
  Incident,
  IncidentStatus,
  Severity,
} from '@/types/alerting';
import { SeverityRail } from '@/viz/SeverityRail';

import {
  COMPARISON_LABEL,
  formatDurationSecs,
  ruleSeverity,
  topThreshold,
} from './alertRuleModel';
import { AlertsSubNav } from './Layout';

type IncidentTab = 'active' | 'unacknowledged' | 'resolved';
type RuleTab = 'all' | 'enabled' | 'disabled';
type RuleDisplayState = 'healthy' | 'pending' | 'firing' | 'disabled';

const SEVERITY_RANK: Record<Severity, number> = {
  info: 0,
  warning: 1,
  error: 2,
  critical: 3,
};

const INCIDENT_TONE: Record<IncidentStatus, PillTone> = {
  open: 'red',
  acknowledged: 'yellow',
  resolved: 'green',
  closed: 'dim',
};

const RULE_STATE_TONE: Record<RuleDisplayState, PillTone> = {
  healthy: 'green',
  pending: 'yellow',
  firing: 'red',
  disabled: 'dim',
};

interface DisplayRule {
  id: string;
  name: string;
  severity: Severity;
  service: string;
  source: string;
  condition: string;
  state: RuleDisplayState;
  lastEvaluation: number | null;
  kind: AlertRuleKind;
  raw: AlertRule;
}

export function Alerts() {
  const location = useLocation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const createAccess = useActionAccess({ permission: 'alerts.manage' });

  React.useEffect(() => {
    if (searchParams.get('create') === '1' && createAccess.allowed) {
      navigate('/alerts/rules/new', { replace: true });
    }
  }, [createAccess.allowed, navigate, searchParams]);

  if (location.pathname.startsWith('/alerts/rules')) return <AlertRulesPage />;
  return <AlertIncidentsPage />;
}

function AlertIncidentsPage() {
  const { t } = useTranslation('alerts');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const acknowledgeAccess = useActionAccess({
    permission: 'alerts.acknowledge',
  });
  const silenceAccess = useActionAccess({ permission: 'alerts.silence' });
  const [tab, setTab] = React.useState<IncidentTab>('active');
  const [viewingIncidentId, setViewingIncidentId] = React.useState<string | null>(null);
  const [silencingIncident, setSilencingIncident] = React.useState<{
    id: string;
    name: string;
  } | null>(null);

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

  const incidents = incidentsQuery.data ?? [];
  const active = incidents.filter(isActiveIncident);
  const unacknowledged = incidents.filter((incident) => incident.status === 'open');
  const resolved = incidents
    .filter(isResolvedIncident)
    .sort((left, right) => (right.resolved_at ?? 0) - (left.resolved_at ?? 0));
  const resolved24h = resolved.filter(
    (incident) =>
      incident.resolved_at !== undefined &&
      Date.now() * 1000 - incident.resolved_at <= 24 * 60 * 60 * 1_000_000,
  );
  const affectedServices = new Set(
    active
      .flatMap((incident) => [
        ...incident.affected_services,
        incident.labels.service,
        incident.labels.svc,
      ])
      .filter((value): value is string => Boolean(value)),
  );
  const rows =
    tab === 'active' ? active : tab === 'unacknowledged' ? unacknowledged : resolved;
  const lastRecovery = resolved[0]?.resolved_at;
  const ruleById = React.useMemo(
    () => new Map((rulesQuery.data ?? []).map((rule) => [rule.id, rule])),
    [rulesQuery.data],
  );

  return (
    <>
      <ListPage
        title={t('center.incidents.title')}
        subtitle={t('center.incidents.subtitle')}
        subnav={<AlertsSubNav />}
        toolbar={<NewRuleActions />}
        kpis={[
          {
            label: t('center.incidents.kpis.active'),
            value: String(active.length),
            sub: severitySummary(active, t),
            tone: active.length > 0 ? 'danger' : 'good',
          },
          {
            label: t('center.incidents.kpis.services'),
            value: String(affectedServices.size),
            sub: t('center.incidents.kpis.services_sub'),
          },
          {
            label: t('center.incidents.kpis.waiting'),
            value: String(unacknowledged.length),
            sub: t('center.incidents.kpis.waiting_sub'),
            tone: unacknowledged.length > 0 ? 'warn' : 'neutral',
          },
          {
            label: t('center.incidents.kpis.recovered'),
            value: String(resolved24h.length),
            sub: lastRecovery
              ? t('center.incidents.kpis.last_recovery', {
                  time: relativeTime(lastRecovery),
                })
              : t('center.incidents.kpis.no_recovery'),
            tone: 'good',
          },
        ]}
        filters={
          <ObjectFilters<IncidentTab>
            value={tab}
            onChange={setTab}
            options={[
              { value: 'active', label: t('center.incidents.tabs.active'), count: active.length },
              {
                value: 'unacknowledged',
                label: t('center.incidents.tabs.unacknowledged'),
                count: unacknowledged.length,
              },
              {
                value: 'resolved',
                label: t('center.incidents.tabs.resolved'),
                count: resolved.length,
              },
            ]}
          />
        }
      >
        {incidentsQuery.isLoading || rulesQuery.isLoading ? (
          <LoadingState variant="list" rows={6} />
        ) : incidentsQuery.isError || rulesQuery.isError ? (
          <ErrorState
            title={t('center.incidents.load_error')}
            error={incidentsQuery.error ?? rulesQuery.error}
            onRetry={() => {
              void incidentsQuery.refetch();
              void rulesQuery.refetch();
            }}
          />
        ) : (
          <div className="space-y-4">
            {active.length === 0 && (
              <HealthyState
                hasRules={(rulesQuery.data?.length ?? 0) > 0}
                recoveredCount={resolved24h.length}
                {...(lastRecovery !== undefined ? { lastRecovery } : {})}
                onViewRecovered={() => setTab('resolved')}
                onViewRules={() => navigate('/alerts/rules')}
                onCreateRule={() => navigate('/alerts/rules/new')}
              />
            )}
            {rows.length > 0 ? (
              <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
                <DataTable
                  rows={rows}
                  rowKey={(incident) => incident.id}
                  onRowClick={(incident) => setViewingIncidentId(incident.id)}
                  columns={[
                    {
                      key: 'severity',
                      header: t('center.incidents.columns.severity'),
                      width: 72,
                      cell: (incident) => (
                        <SeverityRail severity={incident.severity} className="h-row w-[3px]" />
                      ),
                    },
                    {
                      key: 'incident',
                      header: t('center.incidents.columns.incident'),
                      cell: (incident) => (
                        <div className="min-w-0 py-1">
                          <div className="truncate font-strong text-tx-0">{incident.summary}</div>
                          <div className="mt-0.5 truncate text-xs text-tx-3">
                            {incidentService(incident)} · {relativeTime(incident.created_at)}
                          </div>
                        </div>
                      ),
                    },
                    {
                      key: 'status',
                      header: t('center.incidents.columns.status'),
                      width: 120,
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
                      width: 120,
                      cell: (incident) => (
                        <span className="font-mono text-xs text-tx-1">
                          {incidentDuration(incident)}
                        </span>
                      ),
                    },
                    {
                      key: 'owner',
                      header: t('center.incidents.columns.owner'),
                      width: 150,
                      cell: (incident) => (
                        <span className="text-tx-2">
                          {incident.acknowledged_by ??
                            incident.assignees[0] ??
                            t('center.incidents.unassigned')}
                        </span>
                      ),
                    },
                    {
                      key: 'actions',
                      header: t('center.incidents.columns.actions'),
                      width: 250,
                      className: 'overflow-visible text-right',
                      headerClassName: 'text-right',
                      cell: (incident) => {
                        const rule = ruleById.get(incident.rule_id);
                        const runbook =
                          incident.annotations.runbook_url ??
                          incident.annotations.runbook ??
                          rule?.annotations.runbook_url;
                        return (
                          <div
                            className="flex items-center justify-end gap-1"
                            onClick={(event) => event.stopPropagation()}
                          >
                            <TableAction
                              icon={<Check className="h-3.5 w-3.5" />}
                              label={t('center.incidents.actions.ack')}
                              access={restrictActionAccess(
                                acknowledgeAccess,
                                incident.status === 'open',
                                t('drawer.actions.ack_unavailable', {
                                  defaultValue:
                                    'Only open incidents can be acknowledged.',
                                }),
                              )}
                              pending={acknowledge.isPending}
                              onClick={() => acknowledge.mutate(incident.id)}
                            />
                            <TableAction
                              icon={<BellOff className="h-3.5 w-3.5" />}
                              label={t('center.incidents.actions.silence')}
                              access={restrictActionAccess(
                                silenceAccess,
                                isActiveIncident(incident),
                                t('drawer.actions.silence_unavailable', {
                                  defaultValue:
                                    'Only active incidents can be silenced.',
                                }),
                              )}
                              onClick={() =>
                                setSilencingIncident({
                                  id: incident.id,
                                  name: incident.summary,
                                })
                              }
                            />
                            {runbook && (
                              <TableAction
                                icon={<ExternalLink className="h-3.5 w-3.5" />}
                                label={t('center.incidents.actions.runbook')}
                                onClick={() =>
                                  window.open(runbook, '_blank', 'noopener,noreferrer')
                                }
                              />
                            )}
                            <TableAction
                              icon={<Eye className="h-3.5 w-3.5" />}
                              label={t('center.incidents.actions.details')}
                              onClick={() => setViewingIncidentId(incident.id)}
                            />
                          </div>
                        );
                      },
                    },
                  ]}
                />
              </div>
            ) : (
              active.length > 0 && (
                <CompactEmpty
                  title={t(`center.incidents.empty.${tab}.title`)}
                  description={t(`center.incidents.empty.${tab}.description`)}
                />
              )
            )}
          </div>
        )}
      </ListPage>
      <IncidentDetailDrawer
        incidentId={viewingIncidentId}
        onClose={() => setViewingIncidentId(null)}
      />
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

function AlertRulesPage() {
  const { t } = useTranslation('alerts');
  const navigate = useNavigate();
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });
  const [tab, setTab] = React.useState<RuleTab>('all');
  const [search, setSearch] = React.useState('');

  const rulesQuery = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: () => alertsApi.list(),
    refetchInterval: 30_000,
  });
  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'incidents', 'active'],
    queryFn: () => incidentsApi.list(),
    refetchInterval: 30_000,
  });
  const displayRules = React.useMemo(
    () =>
      (rulesQuery.data ?? []).map((rule) =>
        adaptRule(rule, incidentsQuery.data ?? []),
      ),
    [incidentsQuery.data, rulesQuery.data],
  );
  const enabled = displayRules.filter((rule) => rule.raw.enabled);
  const disabled = displayRules.filter((rule) => !rule.raw.enabled);
  const filtered = displayRules.filter((rule) => {
    if (tab === 'enabled' && !rule.raw.enabled) return false;
    if (tab === 'disabled' && rule.raw.enabled) return false;
    const needle = search.trim().toLowerCase();
    return (
      !needle ||
      rule.name.toLowerCase().includes(needle) ||
      rule.service.toLowerCase().includes(needle) ||
      rule.source.toLowerCase().includes(needle)
    );
  });

  return (
    <ListPage
      title={t('center.rules.title')}
      subtitle={t('center.rules.subtitle')}
      subnav={<AlertsSubNav />}
      toolbar={<NewRuleActions />}
      filters={
        <div className="flex w-full flex-col gap-2 sm:flex-row sm:items-center">
          <ObjectFilters<RuleTab>
            value={tab}
            onChange={setTab}
            options={[
              { value: 'all', label: t('center.rules.tabs.all'), count: displayRules.length },
              { value: 'enabled', label: t('center.rules.tabs.enabled'), count: enabled.length },
              { value: 'disabled', label: t('center.rules.tabs.disabled'), count: disabled.length },
            ]}
          />
          <label className="relative ml-auto w-full sm:w-72">
            <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
            <input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t('center.rules.search')}
              className="h-9 w-full rounded-md border border-bd-1 bg-bg-2 pl-9 pr-3 font-sans text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none focus:ring-2 focus:ring-indigo"
            />
          </label>
        </div>
      }
    >
      {rulesQuery.isLoading || incidentsQuery.isLoading ? (
        <LoadingState variant="list" rows={6} />
      ) : rulesQuery.isError || incidentsQuery.isError ? (
        <ErrorState
          title={t('center.rules.load_error')}
          error={rulesQuery.error ?? incidentsQuery.error}
          onRetry={() => {
            void rulesQuery.refetch();
            void incidentsQuery.refetch();
          }}
        />
      ) : displayRules.length === 0 ? (
        <EmptyState
          strategy="create-first"
          title={t('center.rules.empty.title')}
          description={t('center.rules.empty.description')}
          primaryAction={{
            label: t('actions.new_rule'),
            to: '/alerts/rules/new',
            disabled: manageAccess.disabled,
            disabledReason: manageAccess.reason,
          }}
        />
      ) : (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1 px-1 text-xs text-tx-2">
            <span>{t('center.rules.summary.total', { count: displayRules.length })}</span>
            <span className="text-green-soft">
              {t('center.rules.summary.enabled', { count: enabled.length })}
            </span>
            <span className="text-tx-3">
              {t('center.rules.summary.disabled', { count: disabled.length })}
            </span>
          </div>
          {filtered.length === 0 ? (
            <CompactEmpty
              title={t('center.rules.no_match.title')}
              description={t('center.rules.no_match.description')}
            />
          ) : (
            <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
              <DataTable
                rows={filtered}
                rowKey={(rule) => rule.id}
                onRowClick={(rule) =>
                  navigate(
                    rule.kind === 'anomaly'
                      ? `/alerts/anomaly/edit/${rule.id}`
                      : `/alerts/rules/${rule.id}/edit`,
                  )
                }
                isRowClickDisabled={() => manageAccess.disabled}
                rowClickDisabledReason={() => manageAccess.reason}
                columns={[
                  {
                    key: 'rule',
                    header: t('center.rules.columns.rule'),
                    cell: (rule) => (
                      <div className="min-w-0 py-1">
                        <div className="truncate font-strong text-tx-0">{rule.name}</div>
                        <div className="mt-0.5 truncate text-xs text-tx-3">
                          {t(`center.rules.kinds.${rule.kind}`)}
                          {rule.service !== '—' ? ` · ${rule.service}` : ''}
                        </div>
                      </div>
                    ),
                  },
                  {
                    key: 'source',
                    header: t('center.rules.columns.source'),
                    width: 210,
                    cell: (rule) => <span className="text-tx-1">{rule.source}</span>,
                  },
                  {
                    key: 'condition',
                    header: t('center.rules.columns.condition'),
                    width: 220,
                    cell: (rule) => (
                      <span className="font-mono text-xs text-blue-soft">
                        {rule.condition}
                      </span>
                    ),
                  },
                  {
                    key: 'state',
                    header: t('center.rules.columns.state'),
                    width: 120,
                    cell: (rule) => (
                      <Pill tone={RULE_STATE_TONE[rule.state]}>
                        {rule.state === 'firing' && <Dot tone="red" />}
                        {t(`center.rules.state.${rule.state}`)}
                      </Pill>
                    ),
                  },
                  {
                    key: 'lastEvaluation',
                    header: t('center.rules.columns.last_evaluation'),
                    width: 160,
                    cell: (rule) => (
                      <span className="text-tx-3">
                        {rule.lastEvaluation
                          ? relativeTime(rule.lastEvaluation)
                          : t('center.rules.never_evaluated')}
                      </span>
                    ),
                  },
                  {
                    key: 'actions',
                    header: t('center.rules.columns.actions'),
                    width: 84,
                    className: 'overflow-visible text-right',
                    headerClassName: 'text-right',
                    cell: (rule) => (
                      <IconButton
                        disabled={manageAccess.disabled}
                        disabledReason={manageAccess.reason}
                        onClick={(event) => {
                          event.stopPropagation();
                          navigate(
                            rule.kind === 'anomaly'
                              ? `/alerts/anomaly/edit/${rule.id}`
                              : `/alerts/rules/${rule.id}/edit`,
                          );
                        }}
                        className="ml-auto"
                        aria-label={t('actions.edit_rule', { name: rule.name })}
                      >
                        <Pencil className="h-4 w-4" />
                      </IconButton>
                    ),
                  },
                ]}
              />
            </div>
          )}
        </div>
      )}
    </ListPage>
  );
}

function NewRuleActions() {
  const { t } = useTranslation('alerts');
  const navigate = useNavigate();
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });
  return (
    <>
      <ChromeButton onClick={() => navigate('/alerts/silences')}>
        <BellOff className="h-4 w-4" />
        {t('actions.silences')}
      </ChromeButton>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            <Plus className="h-4 w-4" />
            {t('actions.new_rule')}
            <ChevronDown className="h-3.5 w-3.5" />
          </ChromeButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-72">
          <DropdownMenuItem
            className="items-start py-2.5"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onSelect={() => navigate('/alerts/rules/new')}
          >
            <SlidersHorizontal className="mt-0.5 h-4 w-4 shrink-0 text-blue-soft" />
            <span className="min-w-0">
              <span className="block font-strong text-tx-0">
                {t('actions.new_threshold_rule')}
              </span>
              <span className="mt-0.5 block text-xs leading-relaxed text-tx-3">
                {t('actions.new_threshold_rule_description')}
              </span>
            </span>
          </DropdownMenuItem>
          <DropdownMenuItem
            className="items-start py-2.5"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onSelect={() => navigate('/alerts/anomaly/add')}
          >
            <Sparkles className="mt-0.5 h-4 w-4 shrink-0 text-purple-soft" />
            <span className="min-w-0">
              <span className="block font-strong text-tx-0">{t('actions.new_anomaly')}</span>
              <span className="mt-0.5 block text-xs leading-relaxed text-tx-3">
                {t('actions.new_anomaly_description')}
              </span>
            </span>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  );
}

function HealthyState({
  hasRules,
  recoveredCount,
  lastRecovery,
  onViewRecovered,
  onViewRules,
  onCreateRule,
}: {
  hasRules: boolean;
  recoveredCount: number;
  lastRecovery?: number;
  onViewRecovered: () => void;
  onViewRules: () => void;
  onCreateRule: () => void;
}) {
  const { t } = useTranslation('alerts');
  const createAccess = useActionAccess({ permission: 'alerts.manage' });
  if (!hasRules) {
    return (
      <div className="flex min-h-[176px] flex-col items-start justify-center rounded-lg border border-dashed border-bd-1 bg-bg-1 px-6 py-6">
        <div className="font-sans text-base font-display-strong text-tx-0">
          {t('center.incidents.no_rules.title')}
        </div>
        <p className="mt-1 max-w-xl text-sm leading-relaxed text-tx-2">
          {t('center.incidents.no_rules.description')}
        </p>
        <ChromeButton
          variant="primary"
          className="mt-4"
          disabled={createAccess.disabled}
          disabledReason={createAccess.reason}
          onClick={onCreateRule}
        >
          <Plus className="h-4 w-4" />
          {t('center.incidents.no_rules.action')}
        </ChromeButton>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-4 rounded-lg border border-green/30 bg-green-dim px-5 py-4 sm:flex-row sm:items-center">
      <span className="grid h-10 w-10 shrink-0 place-items-center rounded-full bg-green/15 text-green-soft">
        <ShieldCheck className="h-5 w-5" />
      </span>
      <div className="min-w-0 flex-1">
        <div className="font-sans text-base font-display-strong text-tx-0">
          {t('center.incidents.healthy.title')}
        </div>
        <div className="mt-1 text-sm text-tx-2">
          {lastRecovery
            ? t('center.incidents.healthy.with_recovery', {
                count: recoveredCount,
                time: relativeTime(lastRecovery),
              })
            : t('center.incidents.healthy.no_recovery')}
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        <ChromeButton onClick={onViewRecovered}>
          {t('center.incidents.healthy.view_recovered')}
        </ChromeButton>
        <ChromeButton onClick={onViewRules}>
          {t('center.incidents.healthy.view_rules')}
        </ChromeButton>
      </div>
    </div>
  );
}

function ObjectFilters<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (value: T) => void;
  options: Array<{ value: T; label: string; count: number }>;
}) {
  return (
    <div className="flex max-w-full items-center gap-1 overflow-x-auto">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          onClick={() => onChange(option.value)}
          className={`flex h-8 shrink-0 items-center rounded-md px-3 font-sans text-xs font-strong transition-colors ${
            value === option.value
              ? 'bg-bg-4 text-tx-0 shadow-sm'
              : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0'
          }`}
        >
          {option.label}
          <span className="ml-1.5 tabular-nums text-tx-3">{option.count}</span>
        </button>
      ))}
    </div>
  );
}

function CompactEmpty({ title, description }: { title: string; description: string }) {
  return (
    <div className="flex min-h-[160px] flex-col items-center justify-center rounded-lg border border-dashed border-bd-1 bg-bg-1 px-6 text-center">
      <div className="font-sans text-sm font-strong text-tx-0">{title}</div>
      <p className="mt-1 max-w-lg text-sm text-tx-3">{description}</p>
    </div>
  );
}

function TableAction({
  icon,
  label,
  onClick,
  access,
  pending = false,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  access?: ReturnType<typeof useActionAccess>;
  pending?: boolean;
}) {
  const disabled = pending || access?.disabled;
  return (
    <ChromeButton
      size="sm"
      variant="ghost"
      onClick={() => {
        if (!disabled) onClick();
      }}
      disabled={disabled}
      disabledReason={!pending ? access?.reason : undefined}
      aria-label={label}
      title={label}
      className="px-2"
    >
      {icon}
      <span className="hidden min-[1440px]:inline">{label}</span>
    </ChromeButton>
  );
}

function adaptRule(rule: AlertRule, incidents: Incident[]): DisplayRule {
  const activeIncident = incidents.find(
    (incident) => incident.rule_id === rule.id && isActiveIncident(incident),
  );
  const threshold = topThreshold(rule) ?? rule.trigger;
  const duration = threshold.for_periods * rule.query.period_secs;
  const state: RuleDisplayState = !rule.enabled
    ? 'disabled'
    : activeIncident
      ? 'firing'
      : rule.last_state?.kind === 'pending'
        ? 'pending'
        : 'healthy';
  return {
    id: rule.id,
    name: rule.name,
    severity: ruleSeverity(rule),
    service: rule.labels.service ?? rule.labels.svc ?? '—',
    source: rule.query.stream
      ? `${rule.query.stream.name} · ${rule.query.stream.stream_type}`
      : '—',
    condition: `${COMPARISON_LABEL[threshold.operator]} ${threshold.threshold} · ${formatDurationSecs(duration)}`,
    state,
    lastEvaluation: rule.last_eval_at ?? null,
    kind: rule.kind ?? 'scheduled',
    raw: rule,
  };
}

function isActiveIncident(incident: Incident): boolean {
  return incident.status === 'open' || incident.status === 'acknowledged';
}

function isResolvedIncident(incident: Incident): boolean {
  return incident.status === 'resolved' || incident.status === 'closed';
}

function incidentService(incident: Incident): string {
  return (
    incident.affected_services[0] ??
    incident.labels.service ??
    incident.labels.svc ??
    '—'
  );
}

function incidentDuration(incident: Incident): string {
  const end = incident.resolved_at ?? Date.now() * 1000;
  const seconds = Math.max(0, Math.round((end - incident.created_at) / 1_000_000));
  return formatDurationSecs(seconds);
}

function relativeTime(micros: number): string {
  const seconds = Math.max(0, Math.round((Date.now() * 1000 - micros) / 1_000_000));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  if (seconds < 86_400) return `${Math.round(seconds / 3600)}h`;
  if (seconds < 7 * 86_400) return `${Math.round(seconds / 86_400)}d`;
  return formatMicrosActive(micros, false);
}

function severitySummary(
  incidents: Incident[],
  t: ReturnType<typeof useTranslation<'alerts'>>['t'],
): string {
  if (incidents.length === 0) return t('center.incidents.kpis.no_active');
  const counts = incidents.reduce<Record<Severity, number>>(
    (result, incident) => {
      result[incident.severity] += 1;
      return result;
    },
    { info: 0, warning: 0, error: 0, critical: 0 },
  );
  return (Object.entries(counts) as Array<[Severity, number]>)
    .filter(([, count]) => count > 0)
    .sort(([left], [right]) => SEVERITY_RANK[right] - SEVERITY_RANK[left])
    .map(([severity, count]) => `${t(`severity.${severity}`)} ${count}`)
    .join(' · ');
}
