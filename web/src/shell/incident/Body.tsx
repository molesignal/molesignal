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

import { IncidentBlastRadius } from './BlastRadius';
import { IncidentExternalAlert } from './ExternalAlert';
import { IncidentHandlingNote } from './HandlingNote';
import { InhibitionNote } from './InhibitionNote';
import { IncidentMajor } from './MajorIncident';
import { manualCreatorOf } from './origin';
import { IncidentPostmortem } from './Postmortem';
import { IncidentRcaSection } from './Rca';
import { IncidentChanges, IncidentSimilar } from './Related';
import { IncidentRemediation } from './Remediation';
import { IncidentTeam } from './Routing';
import { IncidentRumSessions } from './RumSessions';
import { IncidentSection, KvBlock, SignalRow } from './Section';
import { IncidentTickets } from './Tickets';
import { IncidentTimelineView } from './Timeline';
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
    incident.traceIds.length > 0 || incident.hostIds.length > 0 || incident.affectedServices.length > 0;
  const hasInvestigationContext = Boolean(
    incident.triggeringQuery || hasSignals || incident.labels.service || incident.labels.svc,
  );
  const hasLabels = Object.keys(incident.labels).length > 0;
  const hasAnnotations = Object.keys(incident.annotations).length > 0;
  const creator = manualCreatorOf(incident);
  const annotations = creator?.userId
    ? { ...incident.annotations, created_by: users.nameOf(creator.userId) }
    : incident.annotations;

  const course = (
    <IncidentSection
      title={t('drawer.incident_sections.timeline')}
      icon={<Workflow className="h-3.5 w-3.5" />}
      surface={surface}
    >
      <InhibitionNote incident={incident} />
      <IncidentTimelineView incident={incident} />
    </IncidentSection>
  );
  // The sessions that tell an incident of a RUM rule; nothing for any other.
  const rumSessions = <IncidentRumSessions incident={incident} rule={rule} surface={surface} />;
  // What the sender said of an incident an alert source opened; nothing for any other.
  const external = <IncidentExternalAlert incident={incident} surface={surface} />;
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
  const blastRadius = <IncidentBlastRadius incident={incident} surface={surface} />;
  const similar = <IncidentSimilar incident={incident} surface={surface} />;
  const changes = <IncidentChanges incident={incident} surface={surface} />;
  const remediation = <IncidentRemediation incident={incident} surface={surface} />;
  const major = <IncidentMajor incident={incident} surface={surface} />;
  const postmortem = <IncidentPostmortem incident={incident} surface={surface} />;
  const tickets = <IncidentTickets incident={incident} surface={surface} />;
  const signals = hasSignals && (
    <IncidentSection
      title={t('drawer.incident_sections.signals')}
      icon={<Hash className="h-3.5 w-3.5" />}
      surface={surface}
    >
      {incident.affectedServices.length > 0 && (
        <SignalRow
          label={t('drawer.signals.services')}
          items={incident.affectedServices}
          type="service"
          time={timeWindow}
        />
      )}
      {incident.traceIds.length > 0 && (
        <SignalRow label={t('drawer.signals.traces')} items={incident.traceIds} type="trace_id" time={timeWindow} />
      )}
      {incident.hostIds.length > 0 && (
        <SignalRow label={t('drawer.signals.hosts')} items={incident.hostIds} type="host" time={timeWindow} />
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
      {hasAnnotations && <KvBlock label={t('drawer.annotations.title')} entries={annotations} />}
    </IncidentSection>
  );
  const query = incident.triggeringQuery && (
    <IncidentSection
      title={t('drawer.incident_sections.triggering_query')}
      icon={<Server className="h-3.5 w-3.5" />}
      surface={surface}
    >
      <TriggeringQueryBlock query={incident.triggeringQuery} />
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
      <IncidentTeam incident={incident} />
      <IncidentHandlingNote incident={incident} />
    </IncidentSection>
  );

  if (!surface) {
    return (
      <div className="flex flex-col gap-5">
        {course}
        {major}
        {external}
        {rumSessions}
        {investigation}
        {analysis}
        {changes}
        {remediation}
        {tickets}
        {postmortem}
        {blastRadius}
        {similar}
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
        {major}
        {external}
        {rumSessions}
        {investigation}
        {query}
        {analysis}
        {changes}
        {remediation}
      </div>
      {/* What the incident is: what it touches, what it carries, who has it. */}
      <div className="grid min-w-0 gap-[12px]">
        {tickets}
        {postmortem}
        {blastRadius}
        {similar}
        {signals}
        {metadata}
        {assignees}
      </div>
    </div>
  );
}
