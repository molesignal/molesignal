import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  AgentProfileList,
  AgentProfileRow,
  AgentSettingsSection,
} from './CardlessSurface';

describe('Mole Agent profile cardless surfaces', () => {
  it('uses a divider-led settings section and flat profile rows', () => {
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
    expect(section?.className).not.toMatch(/rounded|shadow|border/);
    expect(section?.querySelector('header')?.className).toContain('border-b');

    const list = container.querySelector('[data-agent-profile-list]');
    expect(list?.className).toContain('divide-y');
    expect(list?.className).not.toMatch(/rounded|shadow|border|grid|gap-/);

    const rows = container.querySelectorAll('[data-agent-profile-row]');
    expect(rows).toHaveLength(2);
    for (const row of rows) {
      expect(row.className).not.toMatch(/rounded|shadow|border/);
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
