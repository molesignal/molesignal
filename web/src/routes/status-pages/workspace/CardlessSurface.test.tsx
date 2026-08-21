import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  StatusPageBand,
  StatusPageCanvas,
  StatusPageFilterBand,
  StatusPageKpiBand,
  StatusPageListSurface,
  StatusPageSection,
} from './CardlessSurface';
import { SettingsCard, SettingsFooter } from './settings/SettingsSection';

describe('Status Pages surface hierarchy', () => {
  it('uses one canvas with gutter-separated KPI, filter and list surfaces', () => {
    const { container } = render(
      <StatusPageCanvas>
        <StatusPageKpiBand
          items={[
            { label: 'Overall', value: 'Operational', tone: 'good' },
            { label: 'Components', value: 3 },
            { label: 'Incidents', value: 0 },
            { label: 'Maintenance', value: 1 },
          ]}
        />
        <StatusPageBand>Actions</StatusPageBand>
        <StatusPageFilterBand>Filters</StatusPageFilterBand>
        <StatusPageListSurface>Rows</StatusPageListSurface>
        <StatusPageSection title="Activity">Events</StatusPageSection>
      </StatusPageCanvas>,
    );

    const canvas = container.querySelector('[data-status-page-canvas]');
    expect(canvas?.className).not.toMatch(/rounded|shadow|border/);

    const kpis = container.querySelector('[data-status-page-kpis]');
    expect(kpis?.className).toContain('gap-[12px]');
    expect(kpis?.className).not.toMatch(/border/);
    expect(kpis?.firstElementChild?.className).toContain('rounded-md');
    expect(kpis?.firstElementChild?.className).toContain('shadow-functional-surface');
    expect(kpis?.firstElementChild?.className).not.toMatch(/\bborder/);

    const band = container.querySelector('[data-status-page-band]');
    expect(band?.className).toContain('rounded-md');
    expect(band?.className).toContain('shadow-functional-surface');
    expect(band?.className).not.toMatch(/\bborder/);

    for (const selector of [
      '[data-status-page-filter-band]',
      '[data-status-page-list-surface]',
    ]) {
      const region = container.querySelector(selector);
      expect(region?.className).toContain('rounded-md');
      expect(region?.className).toContain('shadow-functional-surface');
      expect(region?.className).not.toMatch(/\bborder/);
    }

    const section = container.querySelector('[data-status-page-section]');
    expect(section?.className).toContain('rounded-md');
    expect(section?.className).toContain('shadow-functional-surface');
    expect(section?.className).not.toMatch(/\bborder/);
    expect(section?.firstElementChild?.className).not.toContain('border-b');
  });

  it('uses a borderless settings surface', () => {
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
