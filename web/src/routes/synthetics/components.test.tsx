import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  Section,
  SyntheticsCanvas,
  SyntheticsFilterBar,
  SyntheticsKpiBand,
  SyntheticsListSurface,
} from './components';

describe('Synthetics cardless surfaces', () => {
  it('uses one flat canvas with band and divider-based list structure', () => {
    const { container } = render(
      <SyntheticsCanvas>
        <SyntheticsKpiBand
          columns={3}
          items={[
            { label: 'Platform', value: 2 },
            { label: 'Organization', value: 1 },
            { label: 'Online', value: 3, tone: 'good' },
          ]}
        />
        <SyntheticsListSurface>
          <SyntheticsFilterBar>Filters</SyntheticsFilterBar>
          <div>Rows</div>
        </SyntheticsListSurface>
      </SyntheticsCanvas>,
    );

    const canvas = container.querySelector('[data-synthetics-canvas]');
    expect(canvas?.className).not.toMatch(/rounded|shadow|border/);

    const kpis = container.querySelector('[data-synthetics-kpis]');
    expect(kpis?.className).toContain('border-b');
    expect(kpis?.className).not.toMatch(/rounded|shadow|gap-/);
    expect(kpis?.firstElementChild?.className).not.toMatch(/rounded|shadow|border/);

    const surface = container.querySelector('[data-synthetics-list-surface]');
    expect(surface?.className).toContain('border-b');
    expect(surface?.className).not.toMatch(/rounded|shadow|border-x|border-y/);

    const filters = container.querySelector('[data-synthetics-filter-bar]');
    expect(filters?.className).toContain('min-h-12');
    expect(filters?.className).not.toMatch(/rounded|shadow/);
  });

  it('keeps overview KPI and content sections on the flat canvas', () => {
    const { container } = render(
      <SyntheticsCanvas>
        <SyntheticsKpiBand
          columns={6}
          items={Array.from({ length: 6 }, (_, index) => ({
            label: `KPI ${index + 1}`,
            value: index,
          }))}
        />
        <Section title="Trend" flat>
          Chart
        </Section>
      </SyntheticsCanvas>,
    );

    const kpis = container.querySelector('[data-synthetics-kpis]');
    expect(kpis?.className).toContain('2xl:grid-cols-6');
    expect(kpis?.children).toHaveLength(6);

    const section = container.querySelector('[data-synthetics-section="flat"]');
    expect(section?.className).toContain('border-b');
    expect(section?.className).not.toMatch(/rounded|shadow|border-x|border-y/);
  });
});
