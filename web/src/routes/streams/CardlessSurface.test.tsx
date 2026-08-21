import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  StreamKpiBand,
  StreamSection,
  StreamSettingsSection,
  StreamToggleRow,
} from './CardlessSurface';

describe('Streams surface hierarchy', () => {
  it('renders metrics and sections as borderless surfaces', () => {
    const { container } = render(
      <>
        <StreamKpiBand
          items={[
            { label: 'Rows', value: '24k' },
            { label: 'Storage', value: '8 GiB' },
          ]}
        />
        <StreamSection title="Trend" description="Last 24 hours">
          Chart
        </StreamSection>
      </>,
    );

    const kpis = container.querySelector('[data-stream-kpis]');
    expect(kpis?.className).toContain('gap-[12px]');
    expect(kpis?.className).not.toMatch(/border/);
    expect(kpis?.firstElementChild?.className).toContain('rounded-md');
    expect(kpis?.firstElementChild?.className).toContain('shadow-functional-surface');
    expect(kpis?.firstElementChild?.className).not.toMatch(/\bborder/);

    const section = container.querySelector('[data-stream-section]');
    expect(section?.className).toContain('rounded-md');
    expect(section?.className).toContain('shadow-functional-surface');
    expect(section?.className).not.toMatch(/\bborder/);
    expect(section?.querySelector('header')?.className).not.toContain('border-b');
  });

  it('uses a surface for settings while keeping toggle rows flat', () => {
    const { container } = render(
      <StreamSettingsSection title="Storage" description="Storage behavior">
        <StreamToggleRow title="Store original" checked onChange={() => undefined} />
      </StreamSettingsSection>,
    );

    const section = container.querySelector('[data-stream-settings-section]');
    expect(section?.className).toContain('rounded-md');
    expect(section?.className).toContain('shadow-functional-surface');
    expect(section?.className).not.toMatch(/\bborder/);
    expect(section?.querySelector('header')?.className).not.toContain('border-b');

    const toggle = section?.querySelector('button')?.parentElement;
    expect(toggle?.className).not.toMatch(/rounded|shadow|border|bg-/);
  });
});
