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

describe('Status Pages cardless surfaces', () => {
  it('uses one flat canvas with divider-based KPI, filter and list regions', () => {
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
    expect(kpis?.className).toContain('border-b');
    expect(kpis?.className).not.toMatch(/rounded|shadow|gap-/);
    expect(kpis?.firstElementChild?.className).not.toMatch(/rounded|shadow|border/);

    const band = container.querySelector('[data-status-page-band]');
    expect(band?.className).toContain('border-b');
    expect(band?.className).not.toMatch(/rounded|shadow|border-x|border-y/);

    for (const selector of [
      '[data-status-page-filter-band]',
      '[data-status-page-list-surface]',
    ]) {
      const region = container.querySelector(selector);
      expect(region?.className).not.toMatch(/rounded|shadow|border/);
    }

    const section = container.querySelector('[data-status-page-section]');
    expect(section?.className).not.toMatch(/rounded|shadow|border/);
    expect(section?.firstElementChild?.className).toContain('border-b');
  });

  it('keeps settings sections flat while preserving their internal divider', () => {
    const { container } = render(
      <SettingsCard title="General" description="Status page defaults">
        Settings form
        <SettingsFooter>Save</SettingsFooter>
      </SettingsCard>,
    );

    const section = container.querySelector('[data-status-page-settings-section]');
    expect(section?.className).not.toMatch(/rounded|shadow|border/);
    expect(section?.querySelector('header')?.className).toContain('border-b');
    expect(section?.lastElementChild?.lastElementChild?.className).not.toMatch(/border/);
  });
});
