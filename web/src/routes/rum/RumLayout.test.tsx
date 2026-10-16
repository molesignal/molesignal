import { render } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import { RumDetailPage, RumListPage } from './RumLayout';

function credit(container: HTMLElement) {
  return container.querySelector<HTMLAnchorElement>('[data-rum-geo-credit] a');
}

describe('RUM geolocation credit', () => {
  it('links the foot of every RUM page that shows content to DB-IP', () => {
    const list = render(
      <MemoryRouter initialEntries={['/rum/sessions']}>
        <RumListPage>
          <div>sessions</div>
        </RumListPage>
      </MemoryRouter>,
    );

    expect(credit(list.container)).toHaveAttribute('href', 'https://db-ip.com');
    expect(credit(list.container)).toHaveAttribute('target', '_blank');
    expect(credit(list.container)?.getAttribute('rel')).toContain('noopener');
    list.unmount();

    const detail = render(
      <MemoryRouter initialEntries={['/rum/sessions/view/s-1']}>
        <RumDetailPage title="s-1">
          <div>session</div>
        </RumDetailPage>
      </MemoryRouter>,
    );

    expect(credit(detail.container)).toHaveAttribute('href', 'https://db-ip.com');
  });

  it('leaves it off state screens and the settings, which show no lookup results', () => {
    const empty = render(
      <MemoryRouter initialEntries={['/rum/overview']}>
        <RumListPage state={{ variant: 'empty' }} />
      </MemoryRouter>,
    );

    expect(credit(empty.container)).toBeNull();
    empty.unmount();

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
