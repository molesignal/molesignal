import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ViewTabs } from './ViewTabs';

afterEach(cleanup);

describe('ViewTabs that filter a page in place', () => {
  const items = [
    { key: 'all', label: 'All', count: 5 },
    { key: 'enabled', label: 'Enabled', count: 3 },
  ] as const;

  it('is a named group of buttons, with the selected view pressed', () => {
    render(<ViewTabs label="Rule status" items={items} value="all" onValueChange={vi.fn()} />);

    const group = screen.getByRole('group', { name: 'Rule status' });
    expect(within(group).getByRole('button', { name: 'All 5' })).toHaveAttribute('aria-pressed', 'true');
    expect(within(group).getByRole('button', { name: 'Enabled 3' })).toHaveAttribute('aria-pressed', 'false');
  });

  it('selects a view on click', () => {
    const onValueChange = vi.fn();
    render(<ViewTabs label="Rule status" items={items} value="all" onValueChange={onValueChange} />);

    fireEvent.click(screen.getByRole('button', { name: 'Enabled 3' }));

    expect(onValueChange).toHaveBeenCalledWith('enabled');
  });

  it('shows the selected view on a soft pill, with no track or rule', () => {
    const { container } = render(
      <ViewTabs label="Rule status" items={items} value="enabled" onValueChange={vi.fn()} />,
    );

    expect(container.querySelector('[data-view-pill]')?.className).toContain('bg-indigo-dim');
    expect(container.innerHTML).not.toMatch(/border-b|shadow/);
    expect(screen.getByRole('button', { name: 'Enabled 3' }).className).toContain('text-indigo-soft');
  });
});

describe('ViewTabs that are links', () => {
  it('marks the page the route is on', () => {
    render(
      <MemoryRouter initialEntries={['/rum/performance/web-vitals']}>
        <ViewTabs
          label="Performance"
          items={[
            { key: 'overview', label: 'Overview', to: '/rum/performance/overview' },
            { key: 'vitals', label: 'Web Vitals', to: '/rum/performance/web-vitals' },
          ]}
        />
      </MemoryRouter>,
    );

    const nav = screen.getByRole('navigation', { name: 'Performance' });
    expect(within(nav).getByRole('link', { name: 'Web Vitals' })).toHaveAttribute('aria-current', 'page');
    expect(within(nav).getByRole('link', { name: 'Overview' })).not.toHaveAttribute('aria-current');
  });
});
