import { render } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import type * as productAccess from '@/product/access';

import { RumDetailPage, RumGeoCredit, RumListPage } from './RumLayout';

// The tabs ask what the reader may open.
vi.mock('@/product/access', async (importOriginal) => ({
  ...(await importOriginal<typeof productAccess>()),
  useProductAccess: () => null,
}));

function credit(container: HTMLElement) {
  return container.querySelector<HTMLAnchorElement>('[data-rum-geo-credit] a');
}

describe('RUM geolocation credit', () => {
  it('links to DB-IP, the source of the country of every row', () => {
    const line = render(<RumGeoCredit />);

    expect(credit(line.container)).toHaveAttribute('href', 'https://db-ip.com');
    expect(credit(line.container)).toHaveAttribute('target', '_blank');
    expect(credit(line.container)?.getAttribute('rel')).toContain('noopener');
  });

  it('ends the page of a session, which says where its client was', () => {
    const detail = render(
      <MemoryRouter initialEntries={['/rum/sessions/view/s-1']}>
        <RumDetailPage title="s-1">
          <div>session</div>
        </RumDetailPage>
      </MemoryRouter>,
    );

    expect(credit(detail.container)).toHaveAttribute('href', 'https://db-ip.com');
  });

  it('leaves it off state screens, the applications and the settings, which show no lookup result', () => {
    const empty = render(
      <MemoryRouter initialEntries={['/rum/applications']}>
        <RumListPage state={{ variant: 'empty' }} />
      </MemoryRouter>,
    );

    expect(credit(empty.container)).toBeNull();
    empty.unmount();

    const applications = render(
      <MemoryRouter initialEntries={['/rum/applications']}>
        <RumListPage>
          <div>applications</div>
        </RumListPage>
      </MemoryRouter>,
    );

    expect(credit(applications.container)).toBeNull();
    applications.unmount();

    const settings = render(
      <MemoryRouter initialEntries={['/rum/settings/sdk']}>
        <RumListPage settings>
          <div>sdk</div>
        </RumListPage>
      </MemoryRouter>,
    );

    expect(credit(settings.container)).toBeNull();
  });
});
