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

    const setupLinks = container.querySelectorAll(
      'a[href^="/datasource/recommended/rum"]',
    );
    expect(setupLinks).toHaveLength(4);
    expect(container.querySelector('a[href^="/datasource?"]')).toBeNull();
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
