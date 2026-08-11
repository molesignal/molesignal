import { http } from '@/lib/http';

import type {
  PublicStatusPageSnapshot,
  StatusPageAccessMetadata,
  StatusPageSubscriberChannel,
  StatusPageSubscriptionOutcome,
} from './types';

export async function getPublic(slug: string): Promise<PublicStatusPageSnapshot> {
  const { data } = await http.get<PublicStatusPageSnapshot>(
    `/public/status-pages/${encodeURIComponent(slug)}`,
  );
  return data;
}

export async function getPublicByDomain(): Promise<PublicStatusPageSnapshot | null> {
  const response = await http.get<PublicStatusPageSnapshot>('/public/status-pages/by-domain/current');
  return response.status === 204 ? null : response.data;
}

export async function getAccessMetadata(slug: string): Promise<StatusPageAccessMetadata> {
  const { data } = await http.get<StatusPageAccessMetadata>(
    `/public/status-pages/${encodeURIComponent(slug)}/access`,
  );
  return data;
}

export async function getAccessMetadataByDomain(): Promise<StatusPageAccessMetadata> {
  const { data } = await http.get<StatusPageAccessMetadata>(
    '/public/status-pages/by-domain/access',
  );
  return data;
}

export async function requestPrivateAccess(slug: string, email: string): Promise<void> {
  await http.post(`/public/status-pages/${encodeURIComponent(slug)}/access/request`, { email });
}

export async function requestPrivateAccessByDomain(email: string): Promise<void> {
  await http.post('/public/status-pages/by-domain/access/request', { email });
}

export async function consumePrivateAccess(slug: string, token: string): Promise<void> {
  await http.post(`/public/status-pages/${encodeURIComponent(slug)}/access/consume`, { token });
}

export async function consumePrivateAccessByDomain(token: string): Promise<void> {
  await http.post('/public/status-pages/by-domain/access/consume', { token });
}

export async function subscribePublic(
  slug: string,
  channel: StatusPageSubscriberChannel,
  target: string,
): Promise<StatusPageSubscriptionOutcome> {
  const { data } = await http.post<StatusPageSubscriptionOutcome>(
    `/public/status-pages/${encodeURIComponent(slug)}/subscriptions`,
    { channel, target },
  );
  return data;
}

export async function confirmPublicSubscription(
  slug: string,
  token: string,
): Promise<StatusPageSubscriptionOutcome> {
  const { data } = await http.post<StatusPageSubscriptionOutcome>(
    `/public/status-pages/${encodeURIComponent(slug)}/subscriptions/confirm`,
    { token },
  );
  return data;
}

export async function unsubscribePublic(
  slug: string,
  token: string,
): Promise<StatusPageSubscriptionOutcome> {
  const { data } = await http.post<StatusPageSubscriptionOutcome>(
    `/public/status-pages/${encodeURIComponent(slug)}/subscriptions/unsubscribe`,
    { token },
  );
  return data;
}

export function publicRssUrl(slug: string): string {
  return `/api/v1/public/status-pages/${encodeURIComponent(slug)}/feed.rss`;
}
