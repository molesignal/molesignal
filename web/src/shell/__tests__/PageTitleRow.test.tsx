import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { PageTitleRow } from '@/shell/PageTitleRow';

afterEach(cleanup);

describe('PageTitleRow', () => {
  it('sets the masthead title and its copy on one baseline', () => {
    const { container } = render(
      <PageTitleRow title="Settings" description="Manage everything" level={1} />,
    );

    const row = container.querySelector('[data-page-title-row]');
    expect(row?.getAttribute('data-page-title-size')).toBe('page');
    expect(screen.getByRole('heading', { level: 1, name: 'Settings' }).className).toContain(
      'type-page-title',
    );
    // The separator is part of the inline shape.
    expect(container.querySelector('span[data-page-title-meta]')?.textContent).toBe('·');
    expect(screen.getByText('Manage everything').className).toContain('truncate');
  });

  it('stacks a sub-page description under its title, where it wraps instead of truncating', () => {
    const { container } = render(
      <PageTitleRow
        title="Field masking"
        description="Global rules use first-match order; stream fields can override or disable inherited masking."
        actions={<button type="button">New rule</button>}
        level={2}
        size="section"
      />,
    );

    const row = container.querySelector('[data-page-title-row]');
    expect(row?.getAttribute('data-page-title-size')).toBe('section');

    const heading = screen.getByRole('heading', { level: 2, name: 'Field masking' });
    // One step above the section titles of the form below it.
    expect(heading.className).toContain('type-heading');

    const description = screen.getByText(/Global rules use first-match order/);
    expect(description.className).not.toContain('truncate');
    expect(description.className).toContain('text-pretty');
    // Same block as the title, after it; no inline separator.
    expect(heading.parentElement).toBe(description.parentElement);
    expect(container.querySelector('span[data-page-title-meta]')).toBeNull();
    expect(screen.getByRole('button', { name: 'New rule' })).not.toBeNull();
  });

  it('leaves the description out when there is none', () => {
    const { container } = render(<PageTitleRow title="Users" level={2} size="section" />);
    expect(container.querySelector('p')).toBeNull();
  });
});
