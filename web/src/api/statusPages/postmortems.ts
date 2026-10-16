import { http } from '@/lib/http';

import type { StatusPageIncident, StatusPagePublicationState } from './types';

/** What a resolved incident's customers are told about it afterwards, in Markdown. */
export interface StatusPagePostmortem {
  body: string;
  publication_state: StatusPagePublicationState;
  /** When it was published; cleared when it goes back to a draft. */
  published_at: number | null;
  /** When the published text last changed. */
  revised_at: number | null;
  updated_by: string | null;
  updated_at: number;
}

/** The published postmortem as the public page shows it. */
export interface PublicStatusPagePostmortem {
  body: string;
  published_at: number | null;
  revised_at: number | null;
}

/** The longest postmortem the server keeps, in characters. */
export const MAX_POSTMORTEM_CHARS = 20_000;

const postmortemPath = (pageId: string, eventId: string) =>
  `/status-pages/${encodeURIComponent(pageId)}/incidents/${encodeURIComponent(eventId)}/postmortem`;

/** Saves the text as a draft, or takes a published postmortem back to one. */
export async function savePostmortemDraft(
  pageId: string,
  eventId: string,
  body: string,
): Promise<StatusPageIncident> {
  const { data } = await http.put<StatusPageIncident>(postmortemPath(pageId, eventId), { body });
  return data;
}

/** Publishes the text, or revises the published one; `notify` tells subscribers. */
export async function publishPostmortem(
  pageId: string,
  eventId: string,
  body: string,
  notify: boolean,
): Promise<StatusPageIncident> {
  const { data } = await http.post<StatusPageIncident>(
    `${postmortemPath(pageId, eventId)}/publish`,
    { body, notify },
  );
  return data;
}

export async function unpublishPostmortem(
  pageId: string,
  eventId: string,
): Promise<StatusPageIncident> {
  const { data } = await http.post<StatusPageIncident>(`${postmortemPath(pageId, eventId)}/unpublish`);
  return data;
}

export async function deletePostmortemDraft(
  pageId: string,
  eventId: string,
): Promise<StatusPageIncident> {
  const { data } = await http.delete<StatusPageIncident>(postmortemPath(pageId, eventId));
  return data;
}
