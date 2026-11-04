import { http } from '@/lib/http';

/** The platforms Mole Agent answers in, by their app connector. */
export type ImPlatform = 'feishu' | 'slack' | 'dingtalk' | 'wecom' | 'google_chat' | 'teams';

export const IM_PLATFORMS: readonly ImPlatform[] = [
  'feishu',
  'slack',
  'dingtalk',
  'wecom',
  'google_chat',
  'teams',
];

/** How a bot's long connection is doing: `idle` while it is off or no server holds it yet. */
export type ImBotStatus = 'idle' | 'connecting' | 'connected' | 'error';

/** An app connector that can answer in IM, with its settings and connection. */
export interface ImBot {
  connectorId: string;
  connectorName: string;
  connectorType: string;
  connectorEnabled: boolean;
  platform: ImPlatform | null;
  /** Settings the connector still needs before it can answer, such as a Slack app-level token. */
  missing: string[];
  /** Where Microsoft Teams posts messages (the Azure Bot's messaging endpoint); null for other
   * platforms, and for Teams while the external URL is not https. */
  callbackUrl: string | null;
  enabled: boolean;
  /** New threads in a group become chats of this team when the person asking is in it. */
  defaultTeamId: string | null;
  defaultTeamName: string | null;
  status: ImBotStatus;
  /** Why it is not connected, or who it is connected as. */
  statusDetail: string | null;
  statusAt: number | null;
  lastMessageAt: number | null;
}

export interface ImBots {
  bots: ImBot[];
  /** Without an external URL, people cannot link their IM accounts themselves. */
  externalUrl: boolean;
}

export async function listBots(): Promise<ImBots> {
  const { data } = await http.get<ImBots>('/agent/im/bots');
  return { bots: data.bots ?? [], externalUrl: Boolean(data.externalUrl) };
}

export async function putBot(
  connectorId: string,
  input: { enabled: boolean; defaultTeamId: string | null },
): Promise<ImBot> {
  const { data } = await http.put<ImBot>(
    `/agent/im/bots/${encodeURIComponent(connectorId)}`,
    input,
  );
  return data;
}

export async function removeBot(connectorId: string): Promise<void> {
  await http.delete(`/agent/im/bots/${encodeURIComponent(connectorId)}`);
}

/** The IM account a one-time link would link to the person signed in. */
export interface ImLinkOffer {
  platform: ImPlatform | null;
  connectorName: string;
  externalIdentity: string;
  displayName: string | null;
  expiresAt: number;
}

export async function peekLink(token: string): Promise<ImLinkOffer> {
  const { data } = await http.get<ImLinkOffer>(`/agent/im/links/${encodeURIComponent(token)}`);
  return data;
}

export async function confirmLink(token: string): Promise<{ linked: boolean; endpointId: string }> {
  const { data } = await http.post<{ linked: boolean; endpointId: string }>(
    `/agent/im/links/${encodeURIComponent(token)}`,
  );
  return data;
}
