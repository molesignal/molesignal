import { render, renderHook, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type * as productAccess from './access';
import type { ProductAccess } from './access';
import { FirstOpenablePage, useOpenablePages } from './moduleAccess';

const session = vi.hoisted(() => ({ access: null as unknown }));
vi.mock('./access', async (importOriginal) => ({
  ...(await importOriginal<typeof productAccess>()),
  useProductAccess: () => session.access,
}));

function access(
  routes: ReadonlyArray<readonly [string, boolean]>,
  status: ProductAccess['status'] = 'ready',
): ProductAccess {
  return {
    organizationId: 'acme',
    role: 'Viewer',
    scope: 'organization',
    permissions: new Set() as ProductAccess['permissions'],
    version: 1,
    routeCatalogVersion: 1,
    routes: routes.map(([pathPattern, allowed]) => ({
      id: pathPattern,
      pathPattern: pathPattern,
      allowed,
    })),
    status,
  };
}

const PAGES = ['/alerts/incidents', '/alerts/routing', '/alerts/schedules'] as const;

function openable() {
  return renderHook(() => useOpenablePages(PAGES, (path) => path)).result.current;
}

function Opened() {
  return <div>{useLocation().pathname}</div>;
}

function landing() {
  render(
    <MemoryRouter initialEntries={['/alerts']}>
      <Routes>
        <Route path="/alerts" element={<FirstOpenablePage paths={PAGES} />} />
        <Route path="/alerts/:page" element={<Opened />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe('useOpenablePages', () => {
  beforeEach(() => {
    session.access = null;
  });

  it('leaves out the pages the catalog closes to the reader', () => {
    session.access = access([
      ['/alerts/incidents', false],
      ['/alerts/routing', true],
      ['/alerts/schedules', false],
    ]);

    expect(openable()).toEqual(['/alerts/routing']);
  });

  it('keeps a page the catalog says nothing about, which is for the route guard to decide', () => {
    session.access = access([['/alerts/incidents', false]]);

    expect(openable()).toEqual(['/alerts/routing', '/alerts/schedules']);
  });

  it('offers them all until what the reader holds is known', () => {
    session.access = access([['/alerts/incidents', false]], 'loading');

    expect(openable()).toEqual(PAGES);
  });
});

describe('FirstOpenablePage', () => {
  it('opens on the first page the reader may see', () => {
    session.access = access([
      ['/alerts/incidents', false],
      ['/alerts/routing', false],
      ['/alerts/schedules', true],
    ]);
    landing();

    expect(screen.getByText('/alerts/schedules')).toBeTruthy();
  });

  it('opens on the first page when none is open, so that the guard can say why', () => {
    session.access = access(PAGES.map((path) => [path, false] as const));
    landing();

    expect(screen.getByText('/alerts/incidents')).toBeTruthy();
  });
});
