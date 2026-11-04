import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPage } from '@/api/statusPages';
import i18n from '@/i18n';

import { StatusPageLogoField } from './StatusPageLogoField';

vi.mock('@/api/statusPages', () => ({
  getLogoBlob: vi.fn(),
}));

const getLogoBlob = vi.mocked(statusPagesApi.getLogoBlob);

const page: StatusPage = {
  id: 'page-one',
  orgId: 'org-one',
  name: 'Acme',
  slug: 'acme',
  logoUrl: '/api/v1/public/status-pages/acme/logo/image.png',
  brandColor: '#4F46E5',
  customDomain: null,
  timezone: 'UTC',
  language: 'en-us',
  languages: ['en-us'],
  historyDays: 90,
  deliveryRetentionDays: 90,
  privateSessionDays: 7,
  visibility: 'private',
  lifecycle: 'active',
  archivedAt: null,
  purgeAfter: null,
  createdAt: 1,
  updatedAt: 1,
};

describe.sequential('StatusPageLogoField', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    getLogoBlob.mockReset();
    Object.defineProperty(URL, 'createObjectURL', {
      configurable: true,
      value: vi.fn(() => 'blob:status-page-logo'),
    });
    Object.defineProperty(URL, 'revokeObjectURL', {
      configurable: true,
      value: vi.fn(),
    });
  });

  afterEach(() => cleanup());

  it('accepts the same safe image formats as the backend', () => {
    const onLogoFileChange = vi.fn();
    const onLogoUrlChange = vi.fn();
    const { container } = render(
      <StatusPageLogoField
        active
        page={null}
        logoUrl="https://cdn.example.com/old.svg"
        logoFile={null}
        busy={false}
        onLogoUrlChange={onLogoUrlChange}
        onLogoFileChange={onLogoFileChange}
      />,
    );
    const file = new File(['image'], 'brand.webp', { type: 'image/webp' });
    fireEvent.change(container.querySelector('input[type="file"]') as HTMLInputElement, {
      target: { files: [file] },
    });

    expect(onLogoUrlChange).toHaveBeenCalledWith('');
    expect(onLogoFileChange).toHaveBeenCalledWith(file);
  });

  it('rejects unsupported files before save', () => {
    const onLogoFileChange = vi.fn();
    const { container } = render(
      <StatusPageLogoField
        active
        page={null}
        logoUrl=""
        logoFile={null}
        busy={false}
        onLogoUrlChange={vi.fn()}
        onLogoFileChange={onLogoFileChange}
      />,
    );
    fireEvent.change(container.querySelector('input[type="file"]') as HTMLInputElement, {
      target: { files: [new File(['svg'], 'brand.svg', { type: 'image/svg+xml' })] },
    });

    expect(screen.getByRole('alert').textContent).toContain(
      'Choose a PNG, JPEG, or WebP image.',
    );
    expect(onLogoFileChange).not.toHaveBeenCalled();
  });

  it('uses the tenant-authenticated preview path and supports removal', async () => {
    const user = userEvent.setup();
    const onLogoFileChange = vi.fn();
    const onLogoUrlChange = vi.fn();
    getLogoBlob.mockResolvedValue(new Blob(['image'], { type: 'image/png' }));
    render(
      <StatusPageLogoField
        active
        page={page}
        logoUrl={page.logoUrl ?? ''}
        logoFile={null}
        busy={false}
        onLogoUrlChange={onLogoUrlChange}
        onLogoFileChange={onLogoFileChange}
      />,
    );

    await waitFor(() => expect(getLogoBlob).toHaveBeenCalledWith('page-one'));
    expect(
      (screen.getByRole('textbox', { name: /Logo URL/ }) as HTMLInputElement).value,
    ).toBe('');
    await user.click(screen.getByRole('button', { name: 'Remove logo' }));
    expect(onLogoFileChange).toHaveBeenCalledWith(null);
    expect(onLogoUrlChange).toHaveBeenCalledWith('');
  });
});
