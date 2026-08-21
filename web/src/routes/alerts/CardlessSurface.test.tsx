import { render } from '@testing-library/react';
import { ShieldCheck } from 'lucide-react';
import { describe, expect, it } from 'vitest';

import { AlertFilterTabs, AlertStateBand } from './CardlessSurface';

describe('Alerts surface hierarchy', () => {
  it('uses underline tabs instead of a segmented card', () => {
    const { container } = render(
      <AlertFilterTabs
        value="active"
        onChange={() => undefined}
        options={[
          { value: 'active', label: 'Active', count: 2 },
          { value: 'resolved', label: 'Resolved', count: 4 },
        ]}
      />,
    );

    const root = container.querySelector('[data-alert-filter-tabs]');
    expect(root?.className).not.toMatch(/rounded|shadow|border/);
    const active = root?.querySelector('button');
    expect(active?.className).toContain('border-b-[3px]');
    expect(active?.className).toContain('border-indigo');
    expect(active?.className).not.toMatch(/rounded|shadow/);
  });

  it('renders status content as a borderless tonal surface', () => {
    const { container } = render(
      <AlertStateBand
        icon={ShieldCheck}
        title="Healthy"
        description="No active incidents"
        tone="success"
      />,
    );

    const band = container.querySelector('[data-alert-state-band]');
    expect(band?.className).toContain('bg-green-dim');
    expect(band?.className).toContain('rounded-md');
    expect(band?.className).not.toMatch(/border/);
  });
});
