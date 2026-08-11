import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { StatusPage } from '@/api/statusPages';
import i18n from '@/i18n';

import { PageFormDrawer } from './ConfigurationDrawers';

const page: StatusPage = {
  id: 'page-one',
  org_id: 'org-one',
  name: 'Acme',
  slug: 'acme',
  logo_url: null,
  brand_color: '#4F46E5',
  custom_domain: null,
  timezone: 'UTC',
  language: 'en-us',
  languages: ['en-us'],
  history_days: 90,
  delivery_retention_days: 90,
  private_session_days: 7,
  visibility: 'public',
  lifecycle: 'active',
  archived_at: null,
  purge_after: null,
  created_at: 1,
  updated_at: 1,
};

describe('PageFormDrawer', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
  });

  afterEach(() => cleanup());

  it('edits and submits the configured incident history window', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <PageFormDrawer
        open
        page={page}
        busy={false}
        onOpenChange={vi.fn()}
        onSubmit={onSubmit}
      />,
    );

    const input = screen.getByRole('spinbutton', {
      name: /^Incident history days/,
    });
    expect((input as HTMLInputElement).value).toBe('90');
    fireEvent.change(input, { target: { value: '45' } });
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        input: expect.objectContaining({ history_days: 45 }),
      }),
    );
  });
});
