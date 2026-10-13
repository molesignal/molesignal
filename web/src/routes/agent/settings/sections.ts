/** The sections of the Mole Agent settings, in the order they are offered. */
export const AGENT_SETTINGS_SECTIONS = [
  { value: 'profiles', key: 'settings.tabs.profiles' },
  { value: 'models', key: 'settings.tabs.models' },
  { value: 'prompts', key: 'settings.tabs.prompts' },
  { value: 'tools', key: 'settings.tabs.tools' },
  { value: 'inbound-mcp', key: 'settings.tabs.inbound_mcp' },
  { value: 'data', key: 'settings.tabs.data' },
  { value: 'network', key: 'settings.tabs.network' },
  { value: 'approvals', key: 'settings.tabs.approvals' },
] as const;

export type AgentSettingsSectionKey = (typeof AGENT_SETTINGS_SECTIONS)[number]['value'];

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
