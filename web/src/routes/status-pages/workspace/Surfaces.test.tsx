import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  StatusPageCanvas,
  StatusPageList,
  StatusPageSection,
} from './Surfaces';
import { SettingsCard, SettingsFooter } from './settings/SettingsSection';

describe('Status Pages surface hierarchy', () => {
  it('sets a list and what it is showing on one surface', () => {
    const { container } = render(
      <StatusPageCanvas>
        <StatusPageList head={<span>Views and filters</span>} action={<button type="button">Add</button>}>
          <div data-rows>Rows</div>
        </StatusPageList>
        <StatusPageSection title="Activity">Events</StatusPageSection>
      </StatusPageCanvas>,
    );

    const canvas = container.querySelector('[data-status-page-canvas]');
    expect(canvas?.className).not.toMatch(/rounded|shadow|border/);

    // One card: the head and the rows it filters are not two surfaces with a gutter between.
    const list = container.querySelector('[data-status-page-list]');
    expect(list?.className).toContain('rounded-md');
    expect(list?.className).toContain('shadow-functional-surface');
    expect(list?.className).not.toMatch(/\bborder/);
    const head = list?.querySelector('[data-status-page-list-head]');
    expect(head?.textContent).toBe('Views and filtersAdd');
    expect(head?.nextElementSibling?.hasAttribute('data-rows')).toBe(true);
    // The head is told from the rows by a hairline, not by a surface of its own.
    expect(head?.className).toContain('border-b');
    expect(head?.className).not.toMatch(/shadow|bg-/);

    const section = container.querySelector('[data-status-page-section]');
    expect(section?.className).toContain('rounded-md');
    expect(section?.className).toContain('shadow-functional-surface');
    expect(section?.className).not.toMatch(/\bborder/);
    expect(section?.firstElementChild?.className).not.toContain('border-b');
  });

  it('draws no head for a list that has neither a view nor an action', () => {
    const { container } = render(
      <StatusPageList>
        <div>Rows</div>
      </StatusPageList>,
    );

    expect(container.querySelector('[data-status-page-list-head]')).toBeNull();
  });

  it('uses functional-surface depth for settings', () => {
    const { container } = render(
      <SettingsCard title="General" description="Status page defaults">
        Settings form
        <SettingsFooter>Save</SettingsFooter>
      </SettingsCard>,
    );

    const section = container.querySelector('[data-status-page-settings-section]');
    expect(section?.className).toContain('rounded-md');
    expect(section?.className).toContain('shadow-functional-surface');
    expect(section?.className).not.toMatch(/\bborder/);
    expect(section?.querySelector('header')?.className).not.toContain('border-b');
    expect(section?.lastElementChild?.lastElementChild?.className).not.toMatch(/border/);
  });
});
