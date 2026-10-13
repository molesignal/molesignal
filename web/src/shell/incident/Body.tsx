import { Activity, CircleCheck, Hash, Server, Tag, Workflow } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { Pill } from '@/shell/chrome';
import {
  buildIncidentTimeWindow,
  IncidentInvestigationActions,
} from '@/shell/incident/InvestigationActions';
import { useUsers } from '@/shell/useUsers';
import type { AlertRule, Incident } from '@/types/alerting';

import { IncidentCourse } from './Course';
import { IncidentRcaSection } from './Rca';
import { IncidentSection, KvBlock, SignalRow } from './Section';
import { TriggeringQueryBlock } from './TriggeringQuery';

/**
 * Everything known about one incident: what became of it, where to look, what
 * it touched, what it carries, and the query behind it.
 *
 * The drawer reads it as one column. The incident's own page has the room to
 * set what happened (the course, the ways to investigate, the query, the
 * analysis) beside what the incident is (the signals it touches, its labels,
 * who has it), each part on a surface of its own.
 */
export function IncidentBody({
  incident,
  rule,
  layout = 'column',
}: {
  incident: Incident;
  rule?: AlertRule | undefined;
  layout?: 'column' | 'page';
}) {
  const { t } = useTranslation('alerts');
  const users = useUsers();
  const surface = layout === 'page';
  const timeWindow = React.useMemo(() => buildIncidentTimeWindow(incident), [incident]);
  const hasSignals =
    incident.trace_ids.length > 0 || incident.host_ids.length > 0 || incident.affected_services.length > 0;
  const hasInvestigationContext = Boolean(
    incident.triggering_query || hasSignals || incident.labels.service || incident.labels.svc,
  );
  const hasLabels = Object.keys(incident.labels).length > 0;
  const hasAnnotations = Object.keys(incident.annotations).length > 0;

  const course = (
    <IncidentSection
      title={t('drawer.incident_sections.timeline')}
      icon={<Workflow className="h-3.5 w-3.5" />}
      surface={surface}
    >
      <IncidentCourse incident={incident} />
    </IncidentSection>
  );
  const investigation = hasInvestigationContext && (
    <IncidentSection
      title={t('drawer.incident_sections.investigation')}
      icon={<Activity className="h-3.5 w-3.5" />}
      surface={surface}
    >
      <IncidentInvestigationActions incident={incident} rule={rule} time={timeWindow} />
    </IncidentSection>
  );
  const analysis = <IncidentRcaSection incidentId={incident.id} surface={surface} />;
  const signals = hasSignals && (
    <IncidentSection
      title={t('drawer.incident_sections.signals')}
      icon={<Hash className="h-3.5 w-3.5" />}
      surface={surface}
    >
      {incident.affected_services.length > 0 && (
        <SignalRow
          label={t('drawer.signals.services')}
          items={incident.affected_services}
          type="service"
          time={timeWindow}
        />
      )}
      {incident.trace_ids.length > 0 && (
        <SignalRow label={t('drawer.signals.traces')} items={incident.trace_ids} type="trace_id" time={timeWindow} />
      )}
      {incident.host_ids.length > 0 && (
        <SignalRow label={t('drawer.signals.hosts')} items={incident.host_ids} type="host" time={timeWindow} />
      )}
    </IncidentSection>
  );
  const metadata = (hasLabels || hasAnnotations) && (
    <IncidentSection
      title={t('drawer.incident_sections.metadata')}
      icon={<Tag className="h-3.5 w-3.5" />}
      surface={surface}
    >
      {hasLabels && <KvBlock label={t('drawer.labels.title')} entries={incident.labels} />}
      {hasAnnotations && <KvBlock label={t('drawer.annotations.title')} entries={incident.annotations} />}
    </IncidentSection>
  );
  const query = incident.triggering_query && (
    <IncidentSection
      title={t('drawer.incident_sections.triggering_query')}
      icon={<Server className="h-3.5 w-3.5" />}
      surface={surface}
    >
      <TriggeringQueryBlock query={incident.triggering_query} />
    </IncidentSection>
  );
  const assignees = (
    <IncidentSection
      title={t('drawer.incident_sections.assignees')}
      icon={<CircleCheck className="h-3.5 w-3.5" />}
      surface={surface}
    >
      {incident.assignees.length === 0 ? (
        <p className="font-sans text-xs text-tx-2">{t('drawer.assignees_none')}</p>
      ) : (
        <ul className="flex flex-wrap gap-1.5">
          {incident.assignees.map((assignee) => (
            <li key={assignee}>
              <Pill tone="indigo">{users.nameOf(assignee)}</Pill>
            </li>
          ))}
        </ul>
      )}
    </IncidentSection>
  );

  if (!surface) {
    return (
      <div className="flex flex-col gap-5">
        {course}
        {investigation}
        {analysis}
        {signals}
        {metadata}
        {query}
        {assignees}
      </div>
    );
  }

  return (
    <div
      data-incident-layout="page"
      className="grid grid-cols-1 gap-[12px] xl:grid-cols-[minmax(0,1.7fr)_minmax(0,1fr)] xl:items-start"
    >
      {/* What happened and how to look into it. */}
      <div className="grid min-w-0 gap-[12px]">
        {course}
        {investigation}
        {query}
        {analysis}
      </div>
      {/* What the incident is: what it touches, what it carries, who has it. */}
      <div className="grid min-w-0 gap-[12px]">
        {signals}
        {metadata}
        {assignees}
      </div>
    </div>
  );
}
