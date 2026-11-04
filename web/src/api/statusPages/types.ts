import type { PublicStatusPagePostmortem, StatusPagePostmortem } from './postmortems';
import type { StatusPageSubscriberOrigin, SubscriberComponentScope } from './subscriptions';

export type StatusPageVisibility = 'public' | 'private';
export type StatusPageLifecycle = 'active' | 'archived';
export type StatusPageLanguage = 'en-us' | 'zh-cn';
export type ComponentStatus =
  | 'operational'
  | 'degraded_performance'
  | 'partial_outage'
  | 'major_outage'
  | 'maintenance';
export type ComponentVisibility = 'enabled' | 'hidden';
/** A component, or a group that holds components and shows their worst status. */
export type ComponentKind = 'component' | 'group';
/** What a component depends on: a service name, a synthetic monitor or an alert rule. */
export type DependencyKind = 'technical_service' | 'synthetic_monitor' | 'alert_rule';
/** How much of a dependency's trouble reaches the component. */
export type DependencyImpact = 'outage' | 'degraded';
/** How the health of its dependencies reaches a component. */
export type DependencyMode = 'follow' | 'automation';
export type DependencyStatus = 'operational' | 'degraded_performance' | 'partial_outage' | 'major_outage';

export interface ComponentDependency {
  kind: DependencyKind;
  /** The service name of a technical service, or the id of the monitor or alert rule. */
  target: string;
  impact: DependencyImpact;
}

/** A dependency that is not operational, and why. */
export interface DependencyCause {
  kind: DependencyKind;
  target: string;
  status: DependencyStatus;
  /** The open alert incidents behind it, most severe first, at most five. */
  incidents?: Array<{ id: string; summary: string; severity: AlertSeverity }>;
  incidentCount: number;
  monitorState?: string;
}

/** The health last worked out from a component's dependencies. */
export interface DependencyHealth {
  status: DependencyStatus;
  /** The dependencies that are not operational, worst first. */
  causes: DependencyCause[];
  changedAt: number;
  evaluatedAt: number;
}

/** The most dependencies one component can have. */
export const MAX_COMPONENT_DEPENDENCIES = 100;
export type ComponentLifecycle = 'active' | 'archived';
export type StatusPageIncidentKind = 'incident' | 'maintenance';
export type StatusPagePublicationState = 'draft' | 'published';
export type StatusPageSubscriberChannel = 'email' | 'webhook' | 'slack';
export type StatusPageSubscriberStatus = 'pending' | 'active' | 'unsubscribed';
export type IncidentImpact = 'minor' | 'major' | 'critical' | 'maintenance';
export type PublicIncidentStatus =
  | 'investigating'
  | 'identified'
  | 'in_progress'
  | 'monitoring'
  | 'resolved'
  | 'scheduled'
  | 'completed'
  | 'cancelled';
export type StatusPageEventView =
  | 'current'
  | 'draft'
  | 'resolved'
  | 'upcoming'
  | 'in_progress'
  | 'completed';
export type StatusPageDomainState =
  | 'pending_dns'
  | 'verifying'
  | 'verified'
  | 'provisioning_tls'
  | 'active'
  | 'failed'
  | 'degraded';
export type StatusPageAccessRuleKind = 'email' | 'domain';
export type AutomationSourceKind = 'alert_incident' | 'synthetic_monitor' | 'component_dependencies';
export type AutomationRuleLifecycle = 'draft' | 'active' | 'paused' | 'archived';
export type AutomationPublicationMode = 'approval' | 'automatic';
export type AutomationCandidateState =
  | 'delayed'
  | 'pending_approval'
  | 'approved'
  | 'published'
  | 'rejected'
  | 'cancelled'
  | 'resolved'
  | 'failed';
export type AlertSeverity = 'info' | 'warning' | 'error' | 'critical';

export interface StatusPage {
  id: string;
  orgId: string;
  name: string;
  slug: string;
  logoUrl: string | null;
  brandColor: string;
  customDomain: string | null;
  timezone: string;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  historyDays: number;
  deliveryRetentionDays: number;
  privateSessionDays: number;
  visibility: StatusPageVisibility;
  lifecycle: StatusPageLifecycle;
  archivedAt: number | null;
  purgeAfter: number | null;
  createdAt: number;
  updatedAt: number;
  /** The Slack App message connector visitors add the page to Slack with. */
  slackConnectorId?: string | null;
}

export interface StatusPageComponent {
  id: string;
  orgId: string;
  statusPageId: string;
  name: string;
  description: string;
  status: ComponentStatus;
  visibility: ComponentVisibility;
  lifecycle: ComponentLifecycle;
  position: number;
  archivedAt: number | null;
  createdAt: number;
  updatedAt: number;
  /** What the component depends on; empty when its status is set by hand. */
  dependencies?: ComponentDependency[];
  dependencyMode?: DependencyMode;
  /** Whether a status the component follows tells subscribers. */
  dependencyNotify?: boolean;
  /** The health last worked out from the dependencies. */
  dependencyHealth?: DependencyHealth | null;
  kind?: ComponentKind;
  /** The group the component is in. */
  groupId?: string | null;
}

export interface StatusPageComponentStatusEvent {
  id: string;
  orgId: string;
  statusPageId: string;
  componentId: string;
  status: ComponentStatus;
  startedAt: number;
  endedAt: number | null;
  createdAt: number;
  /** A change made without telling subscribers queued no delivery. */
  notifySubscribers?: boolean;
}

export interface StatusPageIncidentUpdate {
  id: string;
  orgId: string;
  statusPageId: string;
  incidentId: string;
  status: PublicIncidentStatus;
  message: string;
  createdAt: number;
  /** By when this update promised the next one. */
  nextUpdateAt?: number | null;
  /** Whether subscribers were told of this update. */
  notifySubscribers?: boolean;
}

export interface StatusPageIncident {
  id: string;
  orgId: string;
  statusPageId: string;
  sourceIncidentId: string | null;
  kind: StatusPageIncidentKind;
  title: string;
  impact: IncidentImpact;
  status: PublicIncidentStatus;
  publicationState: StatusPagePublicationState;
  draftMessage: string | null;
  componentIds: string[];
  updates: StatusPageIncidentUpdate[];
  startedAt: number;
  endedAt: number | null;
  publishedAt: number | null;
  createdAt: number;
  updatedAt: number;
  /** The postmortem of a resolved incident, draft or published. */
  postmortem?: StatusPagePostmortem | null;
}

export interface StatusPageSnapshot {
  page: StatusPage;
  overallStatus: ComponentStatus;
  components: StatusPageComponent[];
  componentStatusEvents: StatusPageComponentStatusEvent[];
  activeIncidents: StatusPageIncident[];
  scheduledMaintenance: StatusPageIncident[];
  history: StatusPageIncident[];
  updatedAt: number;
  generatedAt: number;
}

export interface StatusPageInput {
  name: string;
  slug: string;
  logoUrl?: string | null;
  brandColor: string;
  timezone: string;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  historyDays: number;
  deliveryRetentionDays: number;
  privateSessionDays: number;
  visibility: StatusPageVisibility;
}

export interface StatusPageComponentInput {
  name: string;
  description?: string;
  status: ComponentStatus;
  visibility: ComponentVisibility;
  lifecycle: ComponentLifecycle;
  position?: number;
  /** Whether a change of status tells subscribers. */
  notify?: boolean;
  /** Fixed once the component is made. */
  kind?: ComponentKind;
  groupId?: string | null;
  /** What the component depends on; left out, it keeps its own. */
  dependencies?: ComponentDependency[];
  dependencyMode?: DependencyMode;
  dependencyNotify?: boolean;
}

export interface StatusPageIncidentInput {
  sourceIncidentId?: string | null;
  kind: StatusPageIncidentKind;
  title: string;
  impact: IncidentImpact;
  status?: PublicIncidentStatus;
  publicationState: StatusPagePublicationState;
  message?: string | null;
  componentIds: string[];
  startedAt?: number;
  /** By when the first published update promises the next one. */
  nextUpdateAt?: number | null;
  /** Whether the first published update tells subscribers. */
  notify?: boolean;
}

export interface StatusPageIncidentUpdateInput {
  status: PublicIncidentStatus;
  message: string;
  nextUpdateAt?: number | null;
  notify?: boolean;
}

export interface StatusPageEventList {
  kind: StatusPageIncidentKind;
  view: StatusPageEventView;
  items: StatusPageIncident[];
  total: number;
}

export interface StatusPageHistoryPage {
  items: StatusPageIncident[];
  page: number;
  perPage: number;
  total: number;
}

export interface StatusPageSubscriber {
  id: string;
  channel: StatusPageSubscriberChannel;
  maskedTarget: string;
  /** The Slack channel and workspace of a subscriber added with Add to Slack. */
  targetLabel?: string | null;
  status: StatusPageSubscriberStatus;
  componentScope?: SubscriberComponentScope;
  componentIds?: string[];
  origin?: StatusPageSubscriberOrigin;
  confirmationSentAt: number | null;
  confirmedAt: number | null;
  unsubscribedAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface StatusPageSubscriberPage {
  items: StatusPageSubscriber[];
  page: number;
  perPage: number;
  total: number;
}

export interface StatusPageDelivery {
  id: string;
  subscriberId: string;
  channel: StatusPageSubscriberChannel;
  maskedTarget: string;
  eventKey: string;
  status: 'pending' | 'processing' | 'delivered' | 'failed';
  attempts: number;
  nextAttemptAt: number;
  lastError: string | null;
  deliveredAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface StatusPageDeliveryPage {
  items: StatusPageDelivery[];
  page: number;
  perPage: number;
  total: number;
}

export interface StatusPageDomainConfig {
  orgId: string;
  statusPageId: string;
  hostname: string;
  verificationToken: string;
  state: StatusPageDomainState;
  domainId: string | null;
  routingValid: boolean;
  lastCheckedAt: number | null;
  lastError: string | null;
  certNotAfter: number | null;
  lastAlertedState: StatusPageDomainState | null;
  lastAlertedAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface StatusPageDomainInstructions {
  config: StatusPageDomainConfig;
  txtName: string;
  txtValue: string;
  routingTarget: string;
}

export interface StatusPageDomainCapability {
  available: boolean;
}

export interface StatusPageAccessRule {
  id: string;
  kind: StatusPageAccessRuleKind;
  maskedValue: string;
  createdAt: number;
  updatedAt: number;
}

export interface StatusPageAccessSession {
  id: string;
  maskedEmail: string;
  originHost: string;
  expiresAt: number;
  lastSeenAt: number;
  createdAt: number;
}

export interface StatusPageSubscriptionOutcome {
  accepted: boolean;
  confirmationRequired: boolean;
}

export interface AutomationMatchers {
  sourceKind: AutomationSourceKind;
  sourceId: string | null;
  labels: Record<string, string>;
  minimumSeverity: AlertSeverity | null;
}

export interface AutomationAction {
  componentIds: string[];
  publicationMode: AutomationPublicationMode;
  sustainedDelaySeconds: number;
  impactMap: { degraded: IncidentImpact; failing: IncidentImpact };
  templates: {
    title: string;
    investigating: string;
    update: string;
    resolved: string;
  };
  correlationKeyTemplate: string;
}

export interface AutomationRuleInput {
  name: string;
  position?: number;
  matchers: AutomationMatchers;
  action: AutomationAction;
}

export interface AutomationApprovalInput {
  title: string;
  impact: IncidentImpact;
  message: string;
  componentIds: string[];
  note?: string;
}

export interface AutomationRule {
  id: string;
  organizationId: string;
  statusPageId: string;
  name: string;
  position: number;
  lifecycle: AutomationRuleLifecycle;
  activeRevisionId: string | null;
  draftRevisionId: string | null;
  createdBy: string;
  createdAt: number;
  updatedAt: number;
}

export interface AutomationRevision {
  id: string;
  organizationId: string;
  statusPageId: string;
  ruleId: string;
  number: number;
  matchers: AutomationMatchers;
  action: AutomationAction;
  contentHash: string;
  createdBy: string;
  createdAt: number;
}

export interface ActiveAutomationRule {
  rule: AutomationRule;
  revision: AutomationRevision;
}

export interface AutomationCandidate {
  id: string;
  organizationId: string;
  statusPageId: string;
  ruleRevisionId: string;
  correlationKey: string;
  state: AutomationCandidateState;
  title: string;
  message: string;
  resolvedMessage: string;
  impact: IncidentImpact;
  componentIds: string[];
  automatic: boolean;
  statusIncidentId: string | null;
  dueAt: number;
  createdAt: number;
  updatedAt: number;
  lastError: string | null;
}

export interface AutomationCandidateSource {
  sourceKind: AutomationSourceKind;
  sourceId: string;
  sourceInstanceId: string;
  severity: AlertSeverity;
  labels: Record<string, string>;
  active: boolean;
  muted: boolean;
  observedAt: number;
  updatedAt: number;
}

export interface AutomationCandidateAction {
  id: string;
  action: string;
  actorId: string | null;
  note: string | null;
  createdAt: number;
}

export interface AutomationWorkItem {
  id: string;
  kind: string;
  status: 'pending' | 'processing' | 'completed' | 'dead_letter';
  attempts: number;
  availableAt: number;
  lastError: string | null;
  completedAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface AutomationCandidateDetail {
  candidate: AutomationCandidate;
  sources: AutomationCandidateSource[];
  actions: AutomationCandidateAction[];
  workItems: AutomationWorkItem[];
}

export interface AutomationSimulationInput {
  sourceKind: AutomationSourceKind;
  sourceId: string;
  sourceInstanceId?: string;
  severity: AlertSeverity;
  labels?: Record<string, string>;
}

export interface AutomationSimulation {
  matchedRuleId: string | null;
  matchedRevisionId: string | null;
  reason: string;
}

export interface PublicStatusPage {
  name: string;
  slug: string;
  logoUrl: string | null;
  brandColor: string;
  timezone: string;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  historyDays: number;
  visibility: StatusPageVisibility;
  /** Whether a reader can add the page to Slack with the installation's Slack app. */
  slackApp?: boolean;
}

export type PublicStatusPageComponent = Pick<
  StatusPageComponent,
  'id' | 'name' | 'description' | 'status' | 'kind' | 'groupId'
>;
export type PublicStatusPageComponentStatusEvent = Pick<
  StatusPageComponentStatusEvent,
  'componentId' | 'status' | 'startedAt' | 'endedAt'
>;
export type PublicStatusPageIncidentUpdate = Pick<
  StatusPageIncidentUpdate,
  'id' | 'status' | 'message' | 'createdAt' | 'nextUpdateAt'
>;
export type PublicStatusPageIncident = Pick<
  StatusPageIncident,
  'id' | 'kind' | 'title' | 'impact' | 'status' | 'componentIds' | 'startedAt' | 'endedAt'
> & {
  updates: PublicStatusPageIncidentUpdate[];
  /** By when the latest update promises the next one, while the event goes on. */
  nextUpdateAt?: number | null;
  /** The published postmortem; a draft is never shown. */
  postmortem?: PublicStatusPagePostmortem | null;
};

export interface PublicStatusPageSnapshot {
  page: PublicStatusPage;
  overallStatus: ComponentStatus;
  components: PublicStatusPageComponent[];
  componentStatusEvents?: PublicStatusPageComponentStatusEvent[];
  activeIncidents: PublicStatusPageIncident[];
  scheduledMaintenance: PublicStatusPageIncident[];
  history: PublicStatusPageIncident[];
  updatedAt?: number;
  generatedAt: number;
}

export interface StatusPageAccessMetadata {
  name: string;
  slug: string;
  logoUrl: string | null;
  brandColor: string;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  visibility: StatusPageVisibility;
}
