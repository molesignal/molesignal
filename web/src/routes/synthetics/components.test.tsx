import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  Section,
  SyntheticsCanvas,
  SyntheticsFilterBar,
  SyntheticsKpiBand,
  SyntheticsListSurface,
  StatePill,
} from './components';

describe('Synthetics surface hierarchy', () => {
  it('uses a canvas with gutter-separated KPI and list surfaces', () => {
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
    expect(kpis?.className).toContain('gap-[12px]');
    expect(kpis?.className).not.toMatch(/border/);
    expect(kpis?.firstElementChild?.className).toContain('rounded-md');
    expect(kpis?.firstElementChild?.className).toContain('shadow-functional-surface');
    expect(kpis?.firstElementChild?.className).not.toMatch(/\bborder/);

    const surface = container.querySelector('[data-synthetics-list-surface]');
    expect(surface?.className).toContain('rounded-md');
    expect(surface?.className).toContain('shadow-functional-surface');
    expect(surface?.className).not.toMatch(/\bborder/);

    const filters = container.querySelector('[data-synthetics-filter-bar]');
    expect(filters?.className).toContain('min-h-12');
    expect(filters?.className).not.toMatch(/border|shadow/);
  });

  it('keeps overview KPI and content sections on the surface canvas', () => {
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
    expect(section?.className).toContain('rounded-md');
    expect(section?.className).toContain('shadow-functional-surface');
    expect(section?.className).not.toMatch(/\bborder/);
  });

  it('renders flaky as a first-class warning state', () => {
    const { container } = render(<StatePill state="flaky" />);

    expect(container.querySelector('span')?.className).toContain('text-orange-soft');
  });
});
