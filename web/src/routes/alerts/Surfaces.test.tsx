import { fireEvent, render, screen, within } from '@testing-library/react';
import { ShieldCheck } from 'lucide-react';
import { describe, expect, it, vi } from 'vitest';

import { AlertFilterTabs, AlertStateBand } from './Surfaces';

describe('Alerts surface hierarchy', () => {
  it('shows alert filters as view tabs with their counts', () => {
    const onChange = vi.fn();
    const { container } = render(
      <AlertFilterTabs
        label="Incident status"
        value="active"
        onChange={onChange}
        options={[
          { value: 'active', label: 'Active', count: 2 },
          { value: 'resolved', label: 'Resolved', count: 4 },
        ]}
      />,
    );

    const root = container.querySelector('[data-alert-filter-tabs]');
    expect(root).not.toBeNull();
    // Flat, like the rest of the alert surfaces: no card, rule or shadow of its own.
    expect(root?.className).not.toMatch(/rounded|shadow|border/);
    const group = screen.getByRole('group', { name: 'Incident status' });
    expect(within(group).getByRole('button', { name: 'Active 2' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    const resolved = within(group).getByRole('button', { name: 'Resolved 4' });
    expect(resolved).toHaveAttribute('aria-pressed', 'false');

    fireEvent.click(resolved);
    expect(onChange).toHaveBeenCalledWith('resolved');
  });

  it('renders status content on a tonal surface', () => {
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
