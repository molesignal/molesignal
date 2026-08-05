import { render } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import { RumSettingsTabs } from '../RumLayout';
import { RumOnboarding } from './Onboarding';

describe('RUM onboarding navigation', () => {
  it('opens the RUM datasource instead of the default Kubernetes datasource', () => {
    const { container } = render(
      <MemoryRouter>
        <RumOnboarding />
      </MemoryRouter>,
    );

    const setupHrefs = Array.from(
      container.querySelectorAll<HTMLAnchorElement>('a[href^="/datasource"]'),
      (link) => link.getAttribute('href'),
    );
    expect(setupHrefs).toEqual([
      '/datasource/recommended/rum',
      '/datasource/recommended/rum-flutter',
      '/datasource/recommended/rum-android',
      '/datasource/recommended/rum-ios',
      '/datasource/recommended/rum?test=1',
    ]);
  });

  it('keeps the SDK settings tab inside RUM settings', () => {
    const { container } = render(
      <MemoryRouter initialEntries={['/rum/settings/sampling']}>
        <RumSettingsTabs />
      </MemoryRouter>,
    );

    expect(
      container.querySelector('a[href="/rum/settings/sdk"]'),
    ).not.toBeNull();
    expect(container.querySelector('a[href^="/datasource"]')).toBeNull();
  });
});
