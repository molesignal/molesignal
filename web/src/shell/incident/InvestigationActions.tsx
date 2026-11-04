import { Activity, ChartNoAxesCombined, FileSearch, MonitorSmartphone } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { incidentRange, rumOpenPath } from '@/routes/rum/alerts/links';
import { incidentFigure } from '@/routes/rum/alerts/samples';
import { buildSignalJumps } from '@/shell/signalReference/links';
import type { AlertRule, Incident } from '@/types/alerting';

export interface IncidentTimeWindow {
  from: string;
  to: string;
}

export interface IncidentInvestigationLink {
  id: 'trigger' | 'logs' | 'metrics' | 'rum';
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
    rum: {
      icon: MonitorSmartphone,
      label: t('drawer.investigation.open_rum'),
      description: t('drawer.investigation.open_rum_description'),
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
            className="group rounded-md bg-bg-2 px-3 py-2.5 outline-none transition-colors hover:bg-bg-3 focus-visible:bg-bg-3"
          >
            <span className="flex items-center gap-2 text-xs font-strong text-tx-0">
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
  // An incident of a RUM rule is about a figure of an application: its statement only names it, and
  // it has no service to pivot on, so the way on is the page of RUM that tells the figure.
  const figure = incidentFigure(incident, rule);
  if (figure) {
    return [{ id: 'rum', to: rumOpenPath(figure, incidentRange(incident, (rule as Partial<AlertRule> | undefined)?.query?.periodSecs)) }];
  }

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
  const fromMs = Math.floor(incident.createdAt / 1_000);
  const toMs = incident.resolvedAt
    ? Math.floor(incident.resolvedAt / 1_000)
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
  const query = incident.triggeringQuery;
  if (!query?.statement.trim()) return null;

  const params = timeParams(time);
  // A rule can come back without its query (an older record, a partial answer); the link is
  // then built from the incident's own statement alone.
  const stream = (rule as Partial<AlertRule> | undefined)?.query?.stream;
  if (stream?.name) params.set('stream', stream.name);
  if (stream?.streamType) params.set('streamType', stream.streamType);

  if (query.language === 'promql') {
    params.set('promql', query.statement.trim());
    return `/metrics?${params.toString()}`;
  }

  params.set('sql', query.statement.trim());
  return stream?.streamType === 'traces'
    ? `/traces?${params.toString()}`
    : `/logs?${params.toString()}`;
}

function incidentSignalJumps(
  incident: Incident,
  time: IncidentTimeWindow,
) {
  const labels = incident.labels;
  const service =
    incident.affectedServices[0] ?? labels.service ?? labels.svc;
  if (service) {
    return buildSignalJumps('service', service, time, { labels });
  }
  const traceId = incident.traceIds[0];
  if (traceId) {
    return buildSignalJumps('trace_id', traceId, time, { labels });
  }
  const host = incident.hostIds[0];
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
