import { Activity, ChartNoAxesCombined, FileSearch } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { buildSignalJumps } from '@/shell/signalReference/links';
import type { AlertRule, Incident } from '@/types/alerting';

export interface IncidentTimeWindow {
  from: string;
  to: string;
}

export interface IncidentInvestigationLink {
  id: 'trigger' | 'logs' | 'metrics';
  to: string;
}

export function IncidentInvestigationActions({
  incident,
  rule,
  time,
}: {
  incident: Incident;
  rule?: AlertRule | undefined;
  time: IncidentTimeWindow;
}) {
  const { t } = useTranslation('alerts');
  const links = buildIncidentInvestigationLinks(incident, rule, time);
  const meta = {
    trigger: {
      icon: Activity,
      label: t('drawer.investigation.open_trigger'),
      description: t('drawer.investigation.open_trigger_description'),
    },
    logs: {
      icon: FileSearch,
      label: t('drawer.investigation.related_logs'),
      description: t('drawer.investigation.related_logs_description'),
    },
    metrics: {
      icon: ChartNoAxesCombined,
      label: t('drawer.investigation.related_metrics'),
      description: t('drawer.investigation.related_metrics_description'),
    },
  } as const;

  return (
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
      {links.map((link) => {
        const item = meta[link.id];
        const Icon = item.icon;
        return (
          <Link
            key={link.id}
            to={link.to}
            className="group rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5 transition-colors hover:border-bd-1 hover:bg-bg-3 focus-visible:bg-indigo-dim focus-visible:text-indigo"
          >
            <span className="flex items-center gap-2 text-xs font-strong text-tx-0 group-focus-visible:text-indigo">
              <Icon className="h-3.5 w-3.5 text-indigo-soft" aria-hidden />
              {item.label}
            </span>
            <span className="mt-1 block text-xs leading-relaxed text-tx-2">
              {item.description}
            </span>
          </Link>
        );
      })}
    </div>
  );
}

export function buildIncidentInvestigationLinks(
  incident: Incident,
  rule: AlertRule | undefined,
  time: IncidentTimeWindow,
): IncidentInvestigationLink[] {
  const links: IncidentInvestigationLink[] = [];
  const trigger = triggeringQueryLink(incident, rule, time);
  if (trigger) links.push({ id: 'trigger', to: trigger });

  const jumps = incidentSignalJumps(incident, time);
  const logs = jumps.find((jump) => jump.id === 'logs');
  const metrics = jumps.find((jump) => jump.id === 'metrics');
  if (logs) links.push({ id: 'logs', to: logs.to });
  if (metrics) links.push({ id: 'metrics', to: metrics.to });
  return links;
}

export function buildIncidentTimeWindow(
  incident: Incident,
): IncidentTimeWindow {
  const fromMs = Math.floor(incident.created_at / 1_000);
  const toMs = incident.resolved_at
    ? Math.floor(incident.resolved_at / 1_000)
    : Date.now();
  return {
    from: new Date(fromMs).toISOString(),
    to: new Date(toMs).toISOString(),
  };
}

function triggeringQueryLink(
  incident: Incident,
  rule: AlertRule | undefined,
  time: IncidentTimeWindow,
): string | null {
  const query = incident.triggering_query;
  if (!query?.statement.trim()) return null;

  const params = timeParams(time);
  const stream = rule?.query.stream;
  if (stream?.name) params.set('stream', stream.name);
  if (stream?.stream_type) params.set('stream_type', stream.stream_type);

  if (query.language === 'promql') {
    params.set('promql', query.statement.trim());
    return `/metrics?${params.toString()}`;
  }

  params.set('sql', query.statement.trim());
  return stream?.stream_type === 'traces'
    ? `/traces?${params.toString()}`
    : `/logs?${params.toString()}`;
}

function incidentSignalJumps(
  incident: Incident,
  time: IncidentTimeWindow,
) {
  const labels = incident.labels;
  const service =
    incident.affected_services[0] ?? labels.service ?? labels.svc;
  if (service) {
    return buildSignalJumps('service', service, time, { labels });
  }
  const traceId = incident.trace_ids[0];
  if (traceId) {
    return buildSignalJumps('trace_id', traceId, time, { labels });
  }
  const host = incident.host_ids[0];
  if (host) {
    return buildSignalJumps('host', host, time, { labels });
  }
  return [];
}

function timeParams(time: IncidentTimeWindow): URLSearchParams {
  return new URLSearchParams({
    from: time.from,
    to: time.to,
    time: `${time.from}..${time.to}`,
  });
}
