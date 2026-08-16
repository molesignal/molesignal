import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import {
  StreamKpiBand,
  StreamSection,
  StreamSettingsSection,
  StreamToggleRow,
} from './CardlessSurface';

describe('Streams cardless surfaces', () => {
  it('renders metrics and sections without outer cards', () => {
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
    expect(kpis?.className).toContain('border-b');
    expect(kpis?.className).not.toMatch(/rounded|shadow/);
    expect(kpis?.firstElementChild?.className).not.toMatch(/rounded|shadow|border/);

    const section = container.querySelector('[data-stream-section]');
    expect(section?.className).not.toMatch(/rounded|shadow|border/);
    expect(section?.querySelector('header')?.className).toContain('border-b');
  });

  it('keeps settings and toggles flat', () => {
    const { container } = render(
      <StreamSettingsSection title="Storage" description="Storage behavior">
        <StreamToggleRow title="Store original" checked onChange={() => undefined} />
      </StreamSettingsSection>,
    );

    const section = container.querySelector('[data-stream-settings-section]');
    expect(section?.className).not.toMatch(/rounded|shadow|border/);
    expect(section?.querySelector('header')?.className).toContain('border-b');

    const toggle = section?.querySelector('button')?.parentElement;
    expect(toggle?.className).not.toMatch(/rounded|shadow|border|bg-/);
  });
});
