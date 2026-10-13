import '@/i18n';

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import type * as productAccess from '@/product/access';
import type { ProductAccess } from '@/product/access';

import { SearchJobsEntry } from './Entry';

const session = vi.hoisted(() => ({ access: null as unknown }));
vi.mock('@/product/access', async (importOriginal) => ({
  ...(await importOriginal<typeof productAccess>()),
  useProductAccess: () => session.access,
}));

function access(permissions: string[], status: ProductAccess['status'] = 'ready'): ProductAccess {
  return {
    organizationId: 'acme-prod',
    role: 'Viewer',
    scope: 'organization',
    permissions: new Set(permissions) as ProductAccess['permissions'],
    version: 1,
    routeCatalogVersion: 1,
    routes: [],
    status,
  };
}

let address = '';
function Address() {
  address = useLocation().pathname;
  return null;
}

function renderEntry() {
  render(
    <MemoryRouter initialEntries={['/streams']}>
      <SearchJobsEntry />
      <Address />
    </MemoryRouter>,
  );
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(cleanup);

describe('SearchJobsEntry', () => {
  it.each([['streams.query'], ['sys.telemetry.read']])(
    'leads to the jobs for a reader who holds %s',
    (permission) => {
      session.access = access([permission]);
      renderEntry();

      fireEvent.click(screen.getByRole('button', { name: 'Search jobs' }));

      expect(address).toBe('/streams/jobs');
    },
  );

  it('is not there for a reader who may read streams but not query them', () => {
    session.access = access(['streams.read', 'pipelines.read']);
    renderEntry();

    expect(screen.queryByRole('button')).toBeNull();
  });

  it('is not there before the permissions are known, or without a session', () => {
    session.access = access(['streams.query'], 'loading');
    renderEntry();
    expect(screen.queryByRole('button')).toBeNull();
    cleanup();

    session.access = null;
    renderEntry();
    expect(screen.queryByRole('button')).toBeNull();
  });
});
