import { http } from '@/lib/http';
import type { Severity } from '@/types/alerting';

/** Where the indexes of the host events stand (`app::node_agent::indexes::IndexState`). */
export type NodeAgentIndexState =
  | { state: 'noStream' }
  | { state: 'missing'; fields: string[] }
  | { state: 'ready' };

/** Where the organization's copy of the host dashboard stands (`builtin::Standing`). */
export type NodeAgentDashboardState = 'missing' | 'outdated' | 'current' | 'taken';

export interface NodeAgentViewStatus {
  key: string;
  id: string;
  installed: boolean;
}

export interface NodeAgentAlertStatus {
  key: string;
  severity: Severity;
  /** The rule the template made; null until the assets are installed. */
  ruleId: string | null;
  enabled: boolean;
}

/** Mirrors `routes::node_agent::Assets`. */
export interface NodeAgentAssets {
  streams: { metrics: boolean; events: boolean };
  indexes: NodeAgentIndexState;
  dashboard: { id: string; state: NodeAgentDashboardState };
  views: NodeAgentViewStatus[];
  /** Null when the reader may not read alert rules. */
  alerts: NodeAgentAlertStatus[] | null;
  /** What the caller was not allowed to install (only in the answer of an install). */
  skipped?: Array<'indexes' | 'dashboard' | 'views' | 'alerts'>;
}

export async function assets(): Promise<NodeAgentAssets> {
  const { data } = await http.get<NodeAgentAssets>('/node-agent/assets');
  return data;
}

/** Installs what is missing; alert rules are made disabled. */
export async function install(): Promise<NodeAgentAssets> {
  const { data } = await http.post<NodeAgentAssets>('/node-agent/assets');
  return data;
}
