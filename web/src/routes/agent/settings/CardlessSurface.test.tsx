import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  AgentProfileList,
  AgentProfileRow,
  AgentSettingsSection,
} from './CardlessSurface';

describe('Mole Agent profile surfaces', () => {
  it('uses tonal settings and profile surfaces without divider lines', () => {
    const { container } = render(
      <AgentSettingsSection
        title="Agent profiles"
        description="Constrained investigation configurations"
        bodyClassName="pt-0"
      >
        <AgentProfileList>
          <AgentProfileRow>Production investigator</AgentProfileRow>
          <AgentProfileRow>On-call responder</AgentProfileRow>
        </AgentProfileList>
      </AgentSettingsSection>,
    );

    const section = container.querySelector('[data-agent-settings-section]');
    expect(section?.className).toContain('rounded-md');
    expect(section?.className).toContain('control-surface');
    expect(section?.className).not.toMatch(/border|shadow/);
    expect(section?.querySelector('header')?.className).not.toContain('border-b');

    const list = container.querySelector('[data-agent-profile-list]');
    expect(list?.className).toContain('space-y-2');
    expect(list?.className).not.toMatch(/border|shadow|divide/);

    const rows = container.querySelectorAll('[data-agent-profile-row]');
    expect(rows).toHaveLength(2);
    for (const row of rows) {
      expect(row.className).toContain('rounded-md');
      expect(row.className).toContain('functional-surface');
      expect(row.className).not.toMatch(/border|shadow/);
    }
  });

  it('can remove the section header divider', () => {
    const { container } = render(
      <AgentSettingsSection
        title="Agent profiles"
        description="Constrained investigation configurations"
        headerDivider={false}
      >
        Profiles
      </AgentSettingsSection>,
    );

    expect(container.querySelector('header')?.className).not.toContain(
      'border-b',
    );
  });
});
