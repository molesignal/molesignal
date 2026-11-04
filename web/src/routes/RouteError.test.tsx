import { cleanup, render, screen } from '@testing-library/react';
import { createMemoryRouter, Outlet, RouterProvider } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { RouteError } from './RouteError';

function Broken({ message = "Cannot read properties of undefined (reading 'trim')" }: { message?: string }): null {
  throw new Error(message);
}

function routerFor(page: React.ReactNode) {
  return createMemoryRouter(
    [
      {
        path: '/',
        element: (
          <div>
            <nav>the sidebar</nav>
            <Outlet />
          </div>
        ),
        errorElement: <RouteError fullPage />,
        children: [{ errorElement: <RouteError />, children: [{ path: 'page', element: page }] }],
      },
    ],
    { initialEntries: ['/page'] },
  );
}

describe('RouteError', () => {
  beforeEach(() => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it('keeps the shell around a page that throws, and says what happened', () => {
    render(
      <TooltipProvider>
        <RouterProvider router={routerFor(<Broken />)} />
      </TooltipProvider>,
    );

    expect(screen.getByText('the sidebar')).not.toBeNull();
    expect(screen.getByTestId('route-error').getAttribute('role')).toBe('alert');
    expect(screen.getByText("Cannot read properties of undefined (reading 'trim')")).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Reload this page' })).not.toBeNull();
    expect(screen.getByRole('link', { name: 'Back to Home' }).getAttribute('href')).toBe('/home');
    expect(screen.getByRole('button', { name: 'Copy error report' })).not.toBeNull();
  });

  it('says a page from an older build only needs a reload', () => {
    render(
      <TooltipProvider>
        <RouterProvider
          router={routerFor(<Broken message="Failed to fetch dynamically imported module: /assets/x.js" />)}
        />
      </TooltipProvider>,
    );

    expect(screen.getByText('A new version is available')).not.toBeNull();
    expect(screen.queryByText(/dynamically imported module/)).toBeNull();
  });
});
