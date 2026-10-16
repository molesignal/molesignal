import { http } from '@/lib/http';

import type { StatusPageComponent } from './types';

/**
 * How the health of a business service reaches the component that stands for
 * it: the component follows the health, or the health is a source of the
 * page's automation rules.
 */
export type ServiceLinkMode = 'follow' | 'automation';

export interface ComponentServiceLink {
  business_service_id: string;
  mode: ServiceLinkMode;
  /** Whether a status the component follows tells subscribers. */
  notify_subscribers: boolean;
}

const linkPath = (pageId: string, componentId: string) =>
  `/status-pages/${encodeURIComponent(pageId)}/components/${encodeURIComponent(componentId)}/business-service`;

export async function linkComponentService(
  pageId: string,
  componentId: string,
  link: ComponentServiceLink,
): Promise<StatusPageComponent> {
  const { data } = await http.put<StatusPageComponent>(linkPath(pageId, componentId), {
    business_service_id: link.business_service_id,
    mode: link.mode,
    notify: link.notify_subscribers,
  });
  return data;
}

export async function unlinkComponentService(
  pageId: string,
  componentId: string,
): Promise<StatusPageComponent> {
  const { data } = await http.delete<StatusPageComponent>(linkPath(pageId, componentId));
  return data;
}
