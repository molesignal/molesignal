import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  AGENT_SETTINGS_SECTIONS,
  agentSettingsPath,
  agentSettingsSection,
  agentSettingsSectionOf,
} from './sections';
import {
  AgentProfileList,
  AgentProfileRow,
  AgentSettingsSection,
} from './Surfaces';

describe('Mole Agent settings surfaces', () => {
  it('opens a section with a line on what it holds, not with its name again', () => {
    const { container } = render(
      <AgentSettingsSection
        title="Agent profiles"
        description="Constrained investigation configurations"
        action={<button type="button">Create profile</button>}
      >
        <AgentProfileList>
          <AgentProfileRow>Production investigator</AgentProfileRow>
          <AgentProfileRow>On-call responder</AgentProfileRow>
        </AgentProfileList>
      </AgentSettingsSection>,
    );

    // The row of sections in the navigation names it: here the name is for the outline only.
    const heading = screen.getByRole('heading', { level: 2, name: 'Agent profiles' });
    expect(heading.className).toContain('sr-only');
    expect(screen.getByText('Constrained investigation configurations')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Create profile' })).toBeTruthy();

    // It lies on the content surface: no panel of its own around it.
    const section = container.querySelector('[data-agent-settings-section]');
    expect(section?.className).not.toMatch(/bg-|border|shadow|rounded/);
    expect(section?.querySelector('header')?.className).not.toContain('border-b');

    const list = container.querySelector('[data-agent-profile-list]');
    expect(list?.className).toContain('space-y-2');
    expect(list?.className).not.toMatch(/border|shadow|divide/);

    // What sets one profile from the next on a white surface is its tint.
    const rows = container.querySelectorAll('[data-agent-profile-row]');
    expect(rows).toHaveLength(2);
    for (const row of rows) {
      expect(row.className).toContain('rounded-md');
      expect(row.className).toContain('control-surface');
      expect(row.className).not.toMatch(/border|shadow/);
    }
  });
});

describe('the sections of the settings', () => {
  it('live under the settings, the first of them at the settings themselves', () => {
    expect(agentSettingsPath('profiles')).toBe('/agent/settings');
    expect(agentSettingsPath('inbound-mcp')).toBe('/agent/settings/inbound-mcp');
    expect(AGENT_SETTINGS_SECTIONS.map((section) => section.value)).toEqual([
      'profiles',
      'models',
      'prompts',
      'tools',
      'inbound-mcp',
      'data',
      'network',
      'approvals',
    ]);
  });

  it('are told from an address, the first one for none or for one that is not known', () => {
    expect(agentSettingsSection('tools')).toBe('tools');
    expect(agentSettingsSection(undefined)).toBe('profiles');
    expect(agentSettingsSection('nope')).toBe('profiles');

    expect(agentSettingsSectionOf('/agent/settings')).toBe('profiles');
    expect(agentSettingsSectionOf('/agent/settings/prompts')).toBe('prompts');
    expect(agentSettingsSectionOf('/agent/settings/prompts/')).toBe('prompts');
    // Not a page of the settings: the navigation shows no row of sections there.
    expect(agentSettingsSectionOf('/agent/chat')).toBeNull();
    expect(agentSettingsSectionOf('/agent/settings/tools/extra')).toBeNull();
  });
});
