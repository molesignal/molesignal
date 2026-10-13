import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

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
  tableSection: 'Remote clusters',
} as const;

afterEach(cleanup);

describe('Settings form layout', () => {
  it('uses canvas-aligned sections and a responsive field grid', () => {
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
    expect(section?.className).not.toContain('rounded-lg');
    expect(section?.className).not.toContain('bg-bg-1');
    expect(section?.className).toContain('[&+&]:border-t');

    const row = screen.getByText(COPY.workspaceName).closest(
      '[data-settings-row]',
    );
    expect(row?.className).toContain('grid-cols-1');
    expect(row?.className).toContain(
      'min-[1100px]:grid-cols-[minmax(220px,280px)_minmax(420px,1fr)]',
    );

    const control = screen.getByRole('textbox', {
      name: COPY.workspaceName,
    }).parentElement;
    expect(control?.className).toContain('min-h-11');
  });

  it('keeps a form to one readable column and lets a table section run wide', () => {
    const { container } = render(
      <>
        <SettingsSection title={COPY.tableSection} measure="full">
          <span>table</span>
        </SettingsSection>
        <SettingsSection title={COPY.workspaceInformation}>
          <SettingsRow label={COPY.workspaceName}>
            <input aria-label={COPY.workspaceName} />
          </SettingsRow>
        </SettingsSection>
      </>,
    );

    const [full, form] = Array.from(
      container.querySelectorAll('[data-settings-section]'),
    ) as HTMLElement[];
    // The hairline between sections spans the page; what is inside is capped.
    expect(form!.firstElementChild?.className).toContain('max-w-[872px]');
    expect(full!.firstElementChild?.className).not.toContain('max-w-[872px]');

    // A field is as wide as its content needs: label 280 + gap 32 + field 560 = the column.
    const control = screen.getByRole('textbox', { name: COPY.workspaceName }).parentElement;
    expect(control?.className).toContain('max-w-[560px]');
  });

  it('steps the type down page → section → topic', () => {
    render(
      <SettingsSection title={COPY.workspaceInformation}>
        <SettingsSubsection title={COPY.basicInformation}>
          <span>{COPY.workspaceName}</span>
        </SettingsSubsection>
      </SettingsSection>,
    );

    // 18px page title (type-heading) > 15px section > 13.5px topic > 12.5px row label.
    expect(screen.getByText(COPY.workspaceInformation).className).toContain('text-lg');
    expect(screen.getByText(COPY.basicInformation).className).toContain('text-md');
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
      'min-[1100px]:grid-cols-[minmax(220px,280px)_minmax(420px,1fr)]',
    );
    expect(screen.getByText(COPY.workspaceValue)).toBeTruthy();
    expect(screen.getByText(COPY.workspaceValue).className).toContain('max-w-[560px]');
  });

  it('groups related topics with whitespace', () => {
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
    expect(topics[1]?.className).not.toContain('[&+&]:border-t');
    expect(topics[1]?.className).toContain('[&+&]:mt-8');
  });
});
