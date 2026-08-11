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
export type ComponentLifecycle = 'active' | 'archived';
export type StatusPageIncidentKind = 'incident' | 'maintenance';
export type StatusPagePublicationState = 'draft' | 'published';
export type StatusPageSubscriberChannel = 'email' | 'webhook';
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

export interface StatusPage {
  id: string;
  org_id: string;
  name: string;
  slug: string;
  logo_url: string | null;
  brand_color: string;
  custom_domain: string | null;
  timezone: string;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  history_days: number;
  delivery_retention_days: number;
  private_session_days: number;
  visibility: StatusPageVisibility;
  lifecycle: StatusPageLifecycle;
  archived_at: number | null;
  purge_after: number | null;
  created_at: number;
  updated_at: number;
}

export interface StatusPageComponent {
  id: string;
  org_id: string;
  status_page_id: string;
  name: string;
  description: string;
  status: ComponentStatus;
  visibility: ComponentVisibility;
  lifecycle: ComponentLifecycle;
  position: number;
  archived_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface StatusPageComponentStatusEvent {
  id: string;
  org_id: string;
  status_page_id: string;
  component_id: string;
  status: ComponentStatus;
  started_at: number;
  ended_at: number | null;
  created_at: number;
}

export interface StatusPageIncidentUpdate {
  id: string;
  org_id: string;
  status_page_id: string;
  incident_id: string;
  status: PublicIncidentStatus;
  message: string;
  created_at: number;
}

export interface StatusPageIncident {
  id: string;
  org_id: string;
  status_page_id: string;
  source_incident_id: string | null;
  kind: StatusPageIncidentKind;
  title: string;
  impact: IncidentImpact;
  status: PublicIncidentStatus;
  publication_state: StatusPagePublicationState;
  draft_message: string | null;
  component_ids: string[];
  updates: StatusPageIncidentUpdate[];
  started_at: number;
  ended_at: number | null;
  published_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface StatusPageSnapshot {
  page: StatusPage;
  overall_status: ComponentStatus;
  components: StatusPageComponent[];
  component_status_events: StatusPageComponentStatusEvent[];
  active_incidents: StatusPageIncident[];
  scheduled_maintenance: StatusPageIncident[];
  history: StatusPageIncident[];
  updated_at: number;
  generated_at: number;
}

export interface StatusPageInput {
  name: string;
  slug: string;
  logo_url?: string | null;
  brand_color: string;
  timezone: string;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  history_days: number;
  delivery_retention_days: number;
  private_session_days: number;
  visibility: StatusPageVisibility;
}

export interface StatusPageComponentInput {
  name: string;
  description?: string;
  status: ComponentStatus;
  visibility: ComponentVisibility;
  lifecycle: ComponentLifecycle;
  position?: number;
}

export interface StatusPageIncidentInput {
  source_incident_id?: string | null;
  kind: StatusPageIncidentKind;
  title: string;
  impact: IncidentImpact;
  status?: PublicIncidentStatus;
  publication_state: StatusPagePublicationState;
  message?: string | null;
  component_ids: string[];
  started_at?: number;
}

export interface StatusPageIncidentUpdateInput {
  status: PublicIncidentStatus;
  message: string;
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
  per_page: number;
  total: number;
}

export interface StatusPageSubscriber {
  id: string;
  channel: StatusPageSubscriberChannel;
  masked_target: string;
  status: StatusPageSubscriberStatus;
  confirmation_sent_at: number | null;
  confirmed_at: number | null;
  unsubscribed_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface StatusPageSubscriberPage {
  items: StatusPageSubscriber[];
  page: number;
  per_page: number;
  total: number;
}

export interface StatusPageDelivery {
  id: string;
  subscriber_id: string;
  channel: StatusPageSubscriberChannel;
  masked_target: string;
  event_key: string;
  status: 'pending' | 'processing' | 'delivered' | 'failed';
  attempts: number;
  next_attempt_at: number;
  last_error: string | null;
  delivered_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface StatusPageDeliveryPage {
  items: StatusPageDelivery[];
  page: number;
  per_page: number;
  total: number;
}

export interface StatusPageDomainConfig {
  org_id: string;
  status_page_id: string;
  hostname: string;
  verification_token: string;
  state: StatusPageDomainState;
  domain_id: string | null;
  routing_valid: boolean;
  last_checked_at: number | null;
  last_error: string | null;
  cert_not_after: number | null;
  last_alerted_state: StatusPageDomainState | null;
  last_alerted_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface StatusPageDomainInstructions {
  config: StatusPageDomainConfig;
  txt_name: string;
  txt_value: string;
  routing_target: string;
}

export interface StatusPageDomainCapability {
  available: boolean;
}

export interface StatusPageAccessRule {
  id: string;
  kind: StatusPageAccessRuleKind;
  masked_value: string;
  created_at: number;
  updated_at: number;
}

export interface StatusPageAccessSession {
  id: string;
  masked_email: string;
  origin_host: string;
  expires_at: number;
  last_seen_at: number;
  created_at: number;
}

export interface StatusPageSubscriptionOutcome {
  accepted: boolean;
  confirmation_required: boolean;
}

export interface PublicStatusPage {
  name: string;
  slug: string;
  logo_url: string | null;
  brand_color: string;
  timezone: string;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  history_days: number;
  visibility: StatusPageVisibility;
}

export type PublicStatusPageComponent = Pick<
  StatusPageComponent,
  'id' | 'name' | 'description' | 'status'
>;
export type PublicStatusPageComponentStatusEvent = Pick<
  StatusPageComponentStatusEvent,
  'component_id' | 'status' | 'started_at' | 'ended_at'
>;
export type PublicStatusPageIncidentUpdate = Pick<
  StatusPageIncidentUpdate,
  'id' | 'status' | 'message' | 'created_at'
>;
export type PublicStatusPageIncident = Pick<
  StatusPageIncident,
  'id' | 'kind' | 'title' | 'impact' | 'status' | 'component_ids' | 'started_at' | 'ended_at'
> & { updates: PublicStatusPageIncidentUpdate[] };

export interface PublicStatusPageSnapshot {
  page: PublicStatusPage;
  overall_status: ComponentStatus;
  components: PublicStatusPageComponent[];
  component_status_events?: PublicStatusPageComponentStatusEvent[];
  active_incidents: PublicStatusPageIncident[];
  scheduled_maintenance: PublicStatusPageIncident[];
  history: PublicStatusPageIncident[];
  updated_at?: number;
  generated_at: number;
}

export interface StatusPageAccessMetadata {
  name: string;
  slug: string;
  logo_url: string | null;
  brand_color: string;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  visibility: StatusPageVisibility;
}
