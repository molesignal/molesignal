import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { StatusPage } from '@/api/statusPages';
import i18n from '@/i18n';

import { PageFormDrawer } from './ConfigurationDrawers';

const page: StatusPage = {
  id: 'page-one',
  orgId: 'org-one',
  name: 'Acme',
  slug: 'acme',
  logoUrl: null,
  brandColor: '#4F46E5',
  customDomain: null,
  timezone: 'UTC',
  language: 'en-us',
  languages: ['en-us'],
  historyDays: 90,
  deliveryRetentionDays: 90,
  privateSessionDays: 7,
  visibility: 'public',
  lifecycle: 'active',
  archivedAt: null,
  purgeAfter: null,
  createdAt: 1,
  updatedAt: 1,
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
        input: expect.objectContaining({ historyDays: 45 }),
      }),
    );
  });
});
