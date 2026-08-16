import type {
  AgentProfile,
  AgentProfileInput,
  NetworkAccess,
  RegisteredTool,
} from '@/api/agent';

import { parseDelimitedList } from '../../editorModel';

export type ProfileEditorSection =
  | 'profile'
  | 'tools'
  | 'data'
  | 'network'
  | 'approvals';

export type ProfileEditorTarget = {
  profile: AgentProfile | 'new';
  section: ProfileEditorSection;
} | null;

export type ProfileEditorPane =
  | 'identity'
  | 'tools'
  | 'data'
  | 'network'
  | 'limits'
  | 'approvals';

export const PROFILE_EDITOR_PANES: readonly ProfileEditorPane[] = [
  'identity',
  'tools',
  'data',
  'network',
  'limits',
  'approvals',
];

export interface ProfileDraft {
  name: string;
  description: string;
  providerId: string;
  model: string;
  allowedTools: string[];
  environments: string[];
  services: string[];
  streams: string[];
  networkAccess: NetworkAccess;
  maxContextTokens: string;
  maxInvestigationMinutes: string;
  maxToolCalls: string;
  l0Policy: string;
  l1Policy: string;
  l2Policy: string;
  l3Policy: string;
  isDefault: boolean;
  enabled: boolean;
}

export function toolsAvailableToAgentProfiles<
  T extends Pick<RegisteredTool, 'available_to_agent'>,
>(tools: readonly T[]): T[] {
  return tools.filter((tool) => tool.available_to_agent);
}

export function profileEditorPane(
  section: ProfileEditorSection,
): ProfileEditorPane {
  return section === 'profile' ? 'identity' : section;
}

export function profileEditorPanes(
  section: ProfileEditorSection,
): readonly ProfileEditorPane[] {
  return section === 'profile'
    ? PROFILE_EDITOR_PANES
    : [profileEditorPane(section)];
}

export function createProfileDraft(
  target: AgentProfile | 'new',
  existingCount: number,
  toolNames: string[],
): ProfileDraft {
  if (target === 'new') {
    return {
      name: '',
      description: '',
      providerId: '',
      model: '',
      allowedTools: toolNames,
      environments: ['development', 'staging', 'production'],
      services: [],
      streams: [],
      networkAccess: 'blocked',
      maxContextTokens: '32000',
      maxInvestigationMinutes: '30',
      maxToolCalls: '32',
      l0Policy: 'automatic',
      l1Policy: 'automatic',
      l2Policy: 'approval',
      l3Policy: 'two_person_approval',
      isDefault: existingCount === 0,
      enabled: true,
    };
  }

  return {
    name: target.name,
    description: target.description,
    providerId: target.model_provider_id ?? '',
    model: target.model ?? '',
    allowedTools: target.allowed_tools.filter((tool) =>
      toolNames.includes(tool),
    ),
    environments: profileScopeValues(target, 'environments'),
    services: profileScopeValues(target, 'services'),
    streams: profileScopeValues(target, 'streams'),
    networkAccess: target.network_access,
    maxContextTokens: String(target.max_context_tokens),
    maxInvestigationMinutes: String(
      Math.max(1, Math.round(target.max_investigation_secs / 60)),
    ),
    maxToolCalls: String(target.max_tool_calls),
    l0Policy: String(target.risk_policy.l0 ?? 'automatic'),
    l1Policy: String(target.risk_policy.l1 ?? 'automatic'),
    l2Policy: String(target.risk_policy.l2 ?? 'approval'),
    l3Policy: String(target.risk_policy.l3 ?? 'two_person_approval'),
    isDefault: target.is_default,
    enabled: target.enabled,
  };
}

export function profileInputFromDraft(
  draft: ProfileDraft,
  defaultDescription: string,
): AgentProfileInput | null {
  const maxContextTokens = Number(draft.maxContextTokens);
  const maxInvestigationMinutes = Number(draft.maxInvestigationMinutes);
  const maxToolCalls = Number(draft.maxToolCalls);
  if (
    !Number.isFinite(maxContextTokens) ||
    maxContextTokens < 1 ||
    !Number.isFinite(maxInvestigationMinutes) ||
    maxInvestigationMinutes < 1 ||
    !Number.isFinite(maxToolCalls) ||
    maxToolCalls < 1 ||
    maxToolCalls > 256
  ) {
    return null;
  }

  return {
    name: draft.name.trim(),
    description: draft.description.trim() || defaultDescription,
    model_provider_id: draft.providerId || null,
    model: draft.model.trim() || null,
    allowed_tools: draft.allowedTools,
    data_scope: {
      environments: draft.environments,
      services: draft.services,
      streams: draft.streams,
      cross_organization: false,
    },
    risk_policy: {
      l0: draft.l0Policy,
      l1: draft.l1Policy,
      l2: draft.l2Policy,
      l3: draft.l3Policy,
    },
    network_access: draft.networkAccess,
    max_context_tokens: Math.round(maxContextTokens),
    max_investigation_secs: Math.round(maxInvestigationMinutes * 60),
    max_tool_calls: Math.round(maxToolCalls),
    is_default: draft.isDefault,
    enabled: draft.enabled,
  };
}

export function profileScopeValues(
  profile: Pick<AgentProfile, 'data_scope'>,
  field: 'environments' | 'services' | 'streams',
): string[] {
  const value = profile.data_scope[field];
  if (Array.isArray(value)) {
    return value.filter((item): item is string => typeof item === 'string');
  }
  return typeof value === 'string' ? parseDelimitedList(value) : [];
}

export function profileRiskPolicy(
  profile: Pick<AgentProfile, 'risk_policy'>,
  risk: 'l0' | 'l1' | 'l2' | 'l3',
): string {
  const value = profile.risk_policy[risk];
  if (typeof value === 'string' && value) return value;
  if (risk === 'l0' || risk === 'l1') return 'automatic';
  return risk === 'l2' ? 'approval' : 'two_person_approval';
}
