import { http } from '@/lib/http';

import type { MessageReference, ReferenceKind } from './chat';

/** Something the person may attach to a question with @. */
export interface ReferenceCandidate extends MessageReference {
  /** Severity and status of an incident, whether a rule is on, a stream's type. */
  detail: string;
}

/** What the person can read whose name has `q` in it, a few of each kind. */
export async function searchReferences(
  q: string,
  kind?: ReferenceKind,
): Promise<ReferenceCandidate[]> {
  const { data } = await http.get<{ results: ReferenceCandidate[] }>('/agent/references', {
    params: { q, ...(kind ? { kind } : {}) },
  });
  return data.results ?? [];
}

export interface IncidentSubject {
  incident: {
    id: string;
    summary: string;
    severity: string;
    status: string;
    created_at_micros: number;
  };
  /** The team the incident's escalation policy names, when the person can start a chat for it. */
  team: { id: string; name: string } | null;
}

/** What starting an investigation of an incident needs: the incident and its team. */
export async function incidentSubject(incidentId: string): Promise<IncidentSubject> {
  const { data } = await http.get<IncidentSubject>(
    `/agent/investigate/incident/${encodeURIComponent(incidentId)}`,
  );
  return data;
}
