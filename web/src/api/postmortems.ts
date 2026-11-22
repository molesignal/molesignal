import { http } from '@/lib/http';

export type PostmortemStatus = 'draft' | 'in_review' | 'published';

/** An incident's internal postmortem: one per incident, written in Markdown. */
export interface Postmortem {
  id: string;
  incidentId: string;
  title: string;
  status: PostmortemStatus;
  body: string;
  ownerId: string | null;
  reviewerIds: string[];
  createdBy: string | null;
  createdAt: number;
  updatedAt: number;
  publishedAt: number | null;
}

export interface ActionItem {
  id: string;
  postmortemId: string;
  incidentId: string;
  title: string;
  ownerId: string | null;
  dueAt: number | null;
  status: 'open' | 'done';
  ticketId: string | null;
  ticketKey: string;
  ticketUrl: string;
  createdAt: number;
  updatedAt: number;
}

export interface PostmortemSummary extends Postmortem {
  incidentSummary: string;
  openActions: number;
  overdueActions: number;
}

export interface PostmortemDetail {
  postmortem: Postmortem | null;
  actions: ActionItem[];
}

export async function list(): Promise<PostmortemSummary[]> {
  const { data } = await http.get<{ postmortems: PostmortemSummary[] }>('/alerts/postmortems');
  return data.postmortems;
}

export async function forIncident(incidentId: string): Promise<PostmortemDetail> {
  const { data } = await http.get<PostmortemDetail>(`/alerts/incidents/${encodeURIComponent(incidentId)}/postmortem`);
  return data;
}

export async function create(incidentId: string, input: { title: string; body: string }): Promise<PostmortemDetail> {
  const { data } = await http.post<PostmortemDetail>(`/alerts/incidents/${encodeURIComponent(incidentId)}/postmortem`, input);
  return data;
}

export async function update(
  id: string,
  patch: { title?: string; body?: string; status?: PostmortemStatus; ownerId?: string | null },
): Promise<PostmortemDetail> {
  const { data } = await http.put<PostmortemDetail>(`/alerts/postmortems/${encodeURIComponent(id)}`, patch);
  return data;
}

export async function addAction(id: string, input: { title: string; ownerId?: string | null; dueAt?: number | null }): Promise<ActionItem> {
  const { data } = await http.post<ActionItem>(`/alerts/postmortems/${encodeURIComponent(id)}/actions`, input);
  return data;
}

export async function updateAction(
  id: string,
  actionId: string,
  patch: { title?: string; ownerId?: string | null; dueAt?: number | null; status?: 'open' | 'done' },
): Promise<ActionItem> {
  const { data } = await http.put<ActionItem>(
    `/alerts/postmortems/${encodeURIComponent(id)}/actions/${encodeURIComponent(actionId)}`,
    patch,
  );
  return data;
}

export async function deleteAction(id: string, actionId: string) {
  await http.delete(`/alerts/postmortems/${encodeURIComponent(id)}/actions/${encodeURIComponent(actionId)}`);
}

export async function actionTicket(id: string, actionId: string, connectorId: string): Promise<ActionItem> {
  const { data } = await http.post<ActionItem>(
    `/alerts/postmortems/${encodeURIComponent(id)}/actions/${encodeURIComponent(actionId)}/ticket`,
    { connectorId: connectorId },
  );
  return data;
}
