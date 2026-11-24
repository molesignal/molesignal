import { http } from '@/lib/http';

/**
 * A business key the organization reads its telemetry by (a tenant, a plan,
 * an experiment group) and the field each signal carries it in. A signal that
 * does not carry it names no field.
 */
export interface BusinessKey {
  /** Lower-case letters, digits and `_`. */
  key: string;
  label: string;
  /** The Span attribute, as the traces stream names the field. */
  span?: string;
  log?: string;
  /** A RUM field, a key of a JSON column included, such as `context.tenant`. */
  rum?: string;
  /** Values that matter most, such as the key customers. */
  important: string[];
}

export interface BusinessContext {
  keys: BusinessKey[];
}

/** The requests of one value of a key, around the time an incident began. */
export interface BusinessValueImpact {
  value: string;
  requests: number;
  errors: number;
  important: boolean;
}

export interface BusinessKeyImpact {
  key: string;
  label: string;
  field: string;
  /** The values with the most failed requests, then the most requests. */
  values: BusinessValueImpact[];
  more: boolean;
}

export interface IncidentBusinessImpact {
  from: number;
  to: number;
  /** The incident's services; none reads every service's requests. */
  services: string[];
  /** Whether any business key the Spans carry is declared. */
  declared: boolean;
  keys: BusinessKeyImpact[];
}

export const businessContextQueryKeys = {
  context: (orgId: string) => ['business-context', orgId] as const,
  incidentImpact: (orgId: string, incidentId: string) => ['business-context', orgId, 'incident', incidentId] as const,
};

export const businessContextApi = {
  async get(): Promise<BusinessContext> {
    const { data } = await http.get<BusinessContext>('/business-context');
    return data;
  },

  async save(context: BusinessContext): Promise<BusinessContext> {
    const { data } = await http.put<BusinessContext>('/business-context', context);
    return data;
  },

  /** For each business key, the values whose requests to the incident's services failed most. */
  async incidentImpact(incidentId: string): Promise<IncidentBusinessImpact> {
    const { data } = await http.get<IncidentBusinessImpact>(
      `/alerts/incidents/${encodeURIComponent(incidentId)}/business-impact`,
    );
    return data;
  },
};
