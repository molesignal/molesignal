import { http } from '@/lib/http';

import type { AlertSeverity, ComponentStatus, ServiceLinkMode } from './statusPages';

/** What a business service depends on. */
export type BusinessServiceDependencyKind =
  | 'technical_service'
  | 'synthetic_monitor'
  | 'alert_rule'
  | 'business_service';
/** How much of a dependency's trouble reaches the service: all of it, or at most degraded performance. */
export type BusinessServiceDependencyImpact = 'outage' | 'degraded';
/** Named like the status of the status-page component that can stand for the service. */
export type BusinessServiceStatus =
  | 'operational'
  | 'degraded_performance'
  | 'partial_outage'
  | 'major_outage';
export type BusinessServiceMonitorState = 'healthy' | 'degraded' | 'failing' | 'flaky' | 'unknown';

export interface BusinessServiceDependency {
  kind: BusinessServiceDependencyKind;
  /** The service name of a technical service, or the id of the monitor, alert rule or business service. */
  target: string;
  impact: BusinessServiceDependencyImpact;
}

export interface BusinessServiceCauseIncident {
  id: string;
  summary: string;
  severity: AlertSeverity;
}

/** A dependency that is not operational, and why. */
export interface BusinessServiceHealthCause {
  kind: BusinessServiceDependencyKind;
  target: string;
  status: BusinessServiceStatus;
  /** The open alert incidents behind it, most severe first, at most five. */
  incidents?: BusinessServiceCauseIncident[];
  incident_count?: number;
  monitor_state?: BusinessServiceMonitorState;
}

/** A period the service spent in one status; the last one is still open. */
export interface BusinessServiceHealthPeriod {
  status: BusinessServiceStatus;
  started_at: number;
  ended_at: number | null;
}

export interface BusinessServiceHealth {
  status: BusinessServiceStatus;
  /** The dependencies that are not operational, worst first. */
  causes: BusinessServiceHealthCause[];
  /** When the status last changed; null before the first evaluation. */
  changed_at: number | null;
  evaluated_at: number | null;
  /** The periods of the last seven days, oldest first. */
  history: BusinessServiceHealthPeriod[];
}

/** A status-page component that stands for the service. */
export interface BusinessServiceLinkedComponent {
  status_page_id: string;
  status_page_name: string;
  component_id: string;
  component_name: string;
  component_status: ComponentStatus;
  mode: ServiceLinkMode;
  notify_subscribers: boolean;
}

export interface BusinessService {
  id: string;
  org_id: string;
  name: string;
  description: string;
  dependencies: BusinessServiceDependency[];
  health: BusinessServiceHealth;
  created_by: string | null;
  created_at: number;
  updated_at: number;
  linked_components: BusinessServiceLinkedComponent[];
}

export interface BusinessServiceInput {
  name: string;
  description: string;
  dependencies: BusinessServiceDependency[];
}

export const MAX_BUSINESS_SERVICE_NAME_CHARS = 120;
export const MAX_BUSINESS_SERVICE_DESCRIPTION_CHARS = 500;
export const MAX_BUSINESS_SERVICE_DEPENDENCIES = 100;

const servicePath = (id: string) => `/business-services/${encodeURIComponent(id)}`;

export async function list(): Promise<BusinessService[]> {
  const { data } = await http.get<BusinessService[]>('/business-services');
  return data;
}

export async function get(id: string): Promise<BusinessService> {
  const { data } = await http.get<BusinessService>(servicePath(id));
  return data;
}

export async function create(input: BusinessServiceInput): Promise<BusinessService> {
  const { data } = await http.post<BusinessService>('/business-services', input);
  return data;
}

export async function update(id: string, input: BusinessServiceInput): Promise<BusinessService> {
  const { data } = await http.put<BusinessService>(servicePath(id), input);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(servicePath(id));
}
