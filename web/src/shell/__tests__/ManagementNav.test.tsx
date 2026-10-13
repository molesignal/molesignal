import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it } from 'vitest';

import { ManagementNav, type ManagementNavGroup } from '@/shell/ManagementNav';

afterEach(cleanup);

const GROUPS: ManagementNavGroup[] = [
  { key: 'org', label: 'Current organization', sections: [{ to: '/settings/general', label: 'General' }] },
  {
    key: 'notify',
    label: 'Notify',
    sections: [
      { to: '/settings/notify/connectors', label: 'Connectors' },
      { to: '/settings/notify/policies', label: 'Policies' },
    ],
  },
  { key: 'security', label: 'Data security', sections: [{ to: '/settings/audit', label: 'Audit' }] },
];

function renderNav(path: string) {
  render(
    <MemoryRouter initialEntries={[path]}>
      <ManagementNav
        ariaLabel="Settings"
        groups={GROUPS}
        currentPath={path}
        searchPlaceholder="Search settings…"
        searchAriaLabel="Search settings"
        noResultsLabel="No matching settings."
        collapseGroupLabel={(group) => `Collapse ${group}`}
        expandGroupLabel={(group) => `Expand ${group}`}
        collapsibleGroups={false}
        mobilePresentation="drawer"
        mobileTriggerLabel="Open settings navigation"
      />
    </MemoryRouter>,
  );
  return within(screen.getByRole('navigation', { name: 'Settings' }));
}

describe('ManagementNav', () => {
  it('sets each group apart as a block: a hairline between blocks, a label on each', () => {
    const nav = renderNav('/settings/notify/connectors');

    for (const label of ['Current organization', 'Notify', 'Data security']) {
      expect(nav.getByText(label)).not.toBeNull();
    }
    // Two hairlines for three blocks; the first block has nothing above it.
    const hairlines = document.querySelectorAll('nav [aria-hidden].h-px');
    expect(hairlines).toHaveLength(2);
  });

  it('names the block that holds the current page a tier darker', () => {
    const nav = renderNav('/settings/notify/connectors');

    expect(nav.getByText('Notify').className).toContain('text-tx-0');
    expect(nav.getByText('Data security').className).toContain('text-tx-2');
    expect(nav.getByText('Current organization').className).toContain('text-tx-2');
  });

  it('marks the current page and parks one glide beside it', () => {
    const nav = renderNav('/settings/notify/connectors');

    expect(nav.getByRole('link', { name: 'Connectors' }).getAttribute('aria-current')).toBe('page');
    expect(nav.getByRole('link', { name: 'Policies' }).getAttribute('aria-current')).toBeNull();
    expect(screen.getAllByTestId('management-nav-indicator')).toHaveLength(1);
  });

  it('keeps the blocks that match a search and drops the rest, with their hairlines', () => {
    const nav = renderNav('/settings/general');

    fireEvent.change(nav.getByRole('textbox', { name: 'Search settings' }), {
      target: { value: 'audit' },
    });

    expect(nav.queryByText('Notify')).toBeNull();
    expect(nav.getByRole('link', { name: 'Audit' })).not.toBeNull();
    expect(document.querySelectorAll('nav [aria-hidden].h-px')).toHaveLength(0);
  });
});
