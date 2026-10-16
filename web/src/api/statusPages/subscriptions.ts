import { http } from '@/lib/http';

import type { StatusPageSubscriberChannel, StatusPageSubscriberStatus } from './types';

/** Whether a subscriber hears about the whole page or only some of its components. */
export type SubscriberComponentScope = 'all' | 'selected';
/** Where a subscriber came from: the page's form, an import, or Add to Slack. */
export type StatusPageSubscriberOrigin = 'page' | 'import' | 'slack_app';
/**
 * How imported addresses start: active at once without being written to, or
 * pending until each confirms an email it is sent.
 */
export type StatusPageSubscriberImportMode = 'active' | 'confirm';

export interface StatusPageImportProblem {
  line: number;
  value: string;
  reason: string;
}

export interface StatusPageSubscriberImportReport {
  imported: number;
  already_subscribed: number;
  skipped_unsubscribed: number;
  confirmations_queued: number;
  /** The rows read, header and blank lines aside. */
  rows: number;
  /** The first rows that were not imported; `problem_count` counts all of them. */
  problems: StatusPageImportProblem[];
  problem_count: number;
  dry_run: boolean;
}

/** What a subscriber sees of their own subscription from a manage link. */
export interface StatusPageSubscriptionPreferences {
  channel: StatusPageSubscriberChannel;
  masked_target: string;
  target_label: string | null;
  status: StatusPageSubscriberStatus;
  component_scope: SubscriberComponentScope;
  component_ids: string[];
}

/** A Slack App message connector, as the settings of a page offer it. */
export interface SlackAppConnectorChoice {
  id: string;
  name: string;
  enabled: boolean;
  /** Whether it holds a client ID and secret, which adding channels needs. */
  adds_channels: boolean;
}

/**
 * The Slack setting of a page: the connector visitors add it to Slack with,
 * the ones it can name, and the redirect URL they need in Slack (`null` until
 * the installation has an HTTPS public address).
 */
export interface StatusPageSlackAppSetting {
  connector_id: string | null;
  redirect_url: string | null;
  connectors: SlackAppConnectorChoice[];
}

/** The largest file an import takes, in bytes. */
export const MAX_SUBSCRIBER_IMPORT_BYTES = 2 * 1024 * 1024;

const publicPath = (slug: string) =>
  `/public/status-pages/${encodeURIComponent(slug)}/subscriptions`;

export async function importSubscribers(
  pageId: string,
  csv: string,
  mode: StatusPageSubscriberImportMode,
  dryRun: boolean,
): Promise<StatusPageSubscriberImportReport> {
  const { data } = await http.post<StatusPageSubscriberImportReport>(
    `/status-pages/${encodeURIComponent(pageId)}/subscribers/import`,
    { csv, mode, dry_run: dryRun },
  );
  return data;
}

export async function getSlackAppSetting(pageId: string): Promise<StatusPageSlackAppSetting> {
  const { data } = await http.get<StatusPageSlackAppSetting>(
    `/status-pages/${encodeURIComponent(pageId)}/slack-app`,
  );
  return data;
}

/** Names the Slack App connector visitors add the page to Slack with; `null` stops offering it. */
export async function setSlackApp(
  pageId: string,
  connectorId: string | null,
): Promise<StatusPageSlackAppSetting> {
  const { data } = await http.put<StatusPageSlackAppSetting>(
    `/status-pages/${encodeURIComponent(pageId)}/slack-app`,
    { connector_id: connectorId },
  );
  return data;
}

export async function readSubscriptionPreferences(
  slug: string,
  token: string,
): Promise<StatusPageSubscriptionPreferences> {
  const { data } = await http.post<StatusPageSubscriptionPreferences>(
    `${publicPath(slug)}/preferences`,
    { token },
  );
  return data;
}

/** Changes what the subscriber hears about; no components means the whole page. */
export async function updateSubscriptionPreferences(
  slug: string,
  token: string,
  componentIds: string[],
): Promise<StatusPageSubscriptionPreferences> {
  const { data } = await http.put<StatusPageSubscriptionPreferences>(
    `${publicPath(slug)}/preferences`,
    { token, component_ids: componentIds },
  );
  return data;
}

/** Starts Add to Slack: the page goes to Slack and comes back with the channel chosen. */
export async function beginSlackSubscription(
  slug: string,
  componentIds: string[],
): Promise<{ authorize_url: string }> {
  const { data } = await http.post<{ authorize_url: string }>(`${publicPath(slug)}/slack`, {
    component_ids: componentIds,
  });
  return data;
}
