import type { PermissionKey } from '@/product/permissions';

/** The sections of the Mole Agent settings, in the order they are offered. */
export const AGENT_SETTINGS_SECTIONS = [
  { value: 'profiles', key: 'settings.tabs.profiles' },
  { value: 'models', key: 'settings.tabs.models' },
  { value: 'usage', key: 'settings.tabs.usage' },
  { value: 'prompts', key: 'settings.tabs.prompts' },
  { value: 'skills', key: 'settings.tabs.skills' },
  { value: 'tools', key: 'settings.tabs.tools' },
  { value: 'inbound-mcp', key: 'settings.tabs.inbound_mcp' },
  { value: 'im', key: 'settings.tabs.im' },
  { value: 'data', key: 'settings.tabs.data' },
  { value: 'network', key: 'settings.tabs.network' },
  { value: 'approvals', key: 'settings.tabs.approvals' },
] as const;

export type AgentSettingsSectionKey = (typeof AGENT_SETTINGS_SECTIONS)[number]['value'];

/** The sections that show and change agent profiles. */
export const PROFILE_SECTIONS: ReadonlySet<AgentSettingsSectionKey> = new Set([
  'profiles',
  'data',
  'network',
  'approvals',
]);

/**
 * What opens a section, where one permission does: without it the section is not there to ask for
 * anything. The models and network sections show what the reader may read of several, skills and
 * tools what members have of their own.
 */
export const AGENT_SETTINGS_READ: Partial<Record<AgentSettingsSectionKey, PermissionKey>> = {
  profiles: 'agent.profiles.read',
  usage: 'agent.usage.read',
  prompts: 'agent.prompts.read',
  'inbound-mcp': 'agent.inbound_mcp.read',
  im: 'agent.im.read',
  data: 'agent.profiles.read',
  approvals: 'agent.profiles.read',
};

/**
 * What changes a section's settings, as its API asks for it. Skills and MCP servers have none: they
 * have team and personal ones members manage themselves.
 */
export const AGENT_SETTINGS_MANAGE: Partial<Record<AgentSettingsSectionKey, PermissionKey>> = {
  profiles: 'agent.profiles.manage',
  models: 'agent.models.manage',
  usage: 'agent.usage.manage',
  prompts: 'agent.prompts.manage',
  'inbound-mcp': 'agent.inbound_mcp.manage',
  im: 'agent.im.manage',
  data: 'agent.profiles.manage',
  network: 'agent.profiles.manage',
  approvals: 'agent.profiles.manage',
};

const FIRST: AgentSettingsSectionKey = 'profiles';

/** Where a section of the settings lives. The first one is the settings' own address. */
export function agentSettingsPath(section: AgentSettingsSectionKey): string {
  return section === FIRST ? '/agent/settings' : `/agent/settings/${section}`;
}

/** The section an address segment names; the first one for none or for one that is not known. */
export function agentSettingsSection(segment: string | undefined): AgentSettingsSectionKey {
  return AGENT_SETTINGS_SECTIONS.find((section) => section.value === segment)?.value ?? FIRST;
}

/** Whether a path is one of the settings', and which section it opens. */
export function agentSettingsSectionOf(pathname: string): AgentSettingsSectionKey | null {
  const match = /^\/agent\/settings(?:\/([^/]+))?\/?$/.exec(pathname);

  return match ? agentSettingsSection(match[1]) : null;
}
