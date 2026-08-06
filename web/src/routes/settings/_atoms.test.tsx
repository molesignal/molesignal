import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  KvRow,
  SettingsGroupStack,
  SettingsRow,
  SettingsSection,
  SettingsSubsection,
} from './_atoms';

const COPY = {
  workspaceInformation: 'Workspace information',
  workspaceName: 'Workspace name',
  workspaceNameDescription: 'Shown throughout the product',
  workspaceId: 'Workspace ID',
  basicInformation: 'Basic information',
  systemIdentity: 'System identity',
  immutable: 'Immutable',
  workspaceValue: 'workspace-123',
} as const;

describe('Settings form layout', () => {
  it('uses light section cards and a responsive field grid', () => {
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

    const section = screen.getByText(COPY.workspaceInformation).closest(
      '[data-settings-section]',
    );
    expect(section?.className).toContain('rounded-lg');
    expect(section?.className).toContain('bg-bg-1');

    const row = screen.getByText(COPY.workspaceName).closest(
      '[data-settings-row]',
    );
    expect(row?.className).toContain('grid-cols-1');
    expect(row?.className).toContain(
      'min-[1100px]:grid-cols-[260px_minmax(420px,1fr)]',
    );

    const control = screen.getByRole('textbox', {
      name: COPY.workspaceName,
    }).parentElement;
    expect(control?.className).toContain('min-h-11');
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
    expect(row?.className).toContain('grid-cols-1');
    expect(row?.className).toContain(
      'min-[1100px]:grid-cols-[260px_minmax(420px,1fr)]',
    );
    expect(screen.getByText(COPY.workspaceValue)).toBeTruthy();
  });

  it('groups related topics with one weak internal divider', () => {
    const { container } = render(
      <SettingsSection
        title={COPY.workspaceInformation}
        contentClassName="gap-0"
      >
        <SettingsSubsection title={COPY.basicInformation}>
          <span>{COPY.workspaceName}</span>
        </SettingsSubsection>
        <SettingsSubsection title={COPY.systemIdentity}>
          <span>{COPY.workspaceId}</span>
        </SettingsSubsection>
      </SettingsSection>,
    );

    expect(container.querySelectorAll('[data-settings-section]')).toHaveLength(1);
    const topics = container.querySelectorAll('[data-settings-subsection]');
    expect(topics).toHaveLength(2);
    expect(topics[1]?.className).toContain('[&+&]:border-t');
  });
});
