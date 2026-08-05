import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  KvRow,
  SettingsGroupStack,
  SettingsRow,
  SettingsSection,
} from './_atoms';

const COPY = {
  workspaceInformation: 'Workspace information',
  workspaceName: 'Workspace name',
  workspaceNameDescription: 'Shown throughout the product',
  workspaceId: 'Workspace ID',
  immutable: 'Immutable',
  workspaceValue: 'workspace-123',
} as const;

describe('Settings form layout', () => {
  it('stacks groups and fields in one column', () => {
    const { container } = render(
      <SettingsGroupStack>
        <SettingsSection title={COPY.workspaceInformation}>
          <SettingsRow
            label={COPY.workspaceName}
            description={COPY.workspaceNameDescription}
          >
            <input aria-label={COPY.workspaceName} />
          </SettingsRow>
        </SettingsSection>
      </SettingsGroupStack>,
    );

    const layout = container.querySelector(
      '[data-settings-layout="single-column"]',
    );
    expect(layout?.className).toContain('flex-col');

    const row = screen.getByText(COPY.workspaceName).closest(
      '[data-settings-row]',
    );
    expect(row?.className).toContain('flex-col');
    expect(row?.className).not.toContain('grid-cols');

    const control = screen.getByRole('textbox', {
      name: COPY.workspaceName,
    }).parentElement;
    expect(control?.className).toContain('max-w-2xl');
  });

  it('uses the same vertical rhythm for read-only metadata', () => {
    render(
      <KvRow label={COPY.workspaceId} hint={COPY.immutable}>
        {COPY.workspaceValue}
      </KvRow>,
    );

    const row = screen.getByText(COPY.workspaceId).closest(
      '[data-settings-row]',
    );
    expect(row?.className).toContain('flex-col');
    expect(row?.className).not.toContain('grid-cols');
    expect(screen.getByText(COPY.workspaceValue)).toBeTruthy();
  });
});
