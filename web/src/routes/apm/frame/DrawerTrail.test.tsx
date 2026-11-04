import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { DrawerTrail, drawerLayers, DrawerTrailProvider, useTrailLabel } from './DrawerTrail';

afterEach(cleanup);

function Search() {
  return <output data-testid="search">{useLocation().search}</output>;
}

function NamedError({ name }: { name: string }) {
  useTrailLabel('error', name);
  return null;
}

const crumb = (key: string) => document.querySelector<HTMLButtonElement>(`[data-apm-drawer-crumb="${key}"]`)!;

function renderTrail(search: string, over: 'endpoint' | 'error' | 'trace', onLeave = vi.fn()) {
  render(
    <MemoryRouter initialEntries={[`/apm/services/checkout${search}`]}>
      <DrawerTrailProvider>
        <NamedError name="PaymentDeclined" />
        <DrawerTrail over={over} onLeave={onLeave} />
        <Search />
      </DrawerTrailProvider>
    </MemoryRouter>,
  );
  return onLeave;
}

describe('the trail of a drawer opened over others', () => {
  it('names the endpoint and the error under a trace, the first first', () => {
    renderTrail('?endpoint=POST+%2Forders&kind=http&error=fp-checkout', 'trace');

    const crumbs = screen.getAllByRole('button');
    expect(crumbs.map((crumb) => crumb.getAttribute('data-apm-drawer-crumb'))).toEqual(['endpoint', 'error']);
    expect(crumbs[0]).toHaveTextContent('POST /orders');
    expect(crumbs[1]).toHaveTextContent('PaymentDeclined');
  });

  it('goes back to the error by closing only the trace', () => {
    const onLeave = renderTrail('?endpoint=POST+%2Forders&error=fp-checkout', 'trace');

    fireEvent.click(crumb('error'));

    expect(onLeave).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId('search').textContent).toContain('error=fp-checkout');
  });

  it('goes back to the endpoint by closing every drawer over it', () => {
    const onLeave = renderTrail('?endpoint=POST+%2Forders&error=fp-checkout', 'trace');

    fireEvent.click(crumb('endpoint'));

    expect(onLeave).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId('search').textContent).not.toContain('error=');
    expect(screen.getByTestId('search').textContent).toContain('endpoint=');
  });

  it('leaves the error out of its own trail, and says nothing with no drawer under', () => {
    renderTrail('?endpoint=POST+%2Forders&error=fp-checkout', 'error');
    expect(screen.getAllByRole('button').map((crumb) => crumb.getAttribute('data-apm-drawer-crumb'))).toEqual(['endpoint']);
    cleanup();

    renderTrail('?error=fp-checkout', 'error');
    expect(screen.queryByRole('navigation')).toBeNull();
  });

  it('stacks the drawers in the order their keys stand in the address', () => {
    expect(drawerLayers(new URLSearchParams('namespace=shop&endpoint=GET+%2Fx&kind=http&error=fp'))).toEqual(['endpoint', 'error']);
    expect(drawerLayers(new URLSearchParams('error=fp&endpoint=GET+%2Fx&kind=http'))).toEqual(['error', 'endpoint']);
    expect(drawerLayers(new URLSearchParams('error=&endpoint=GET+%2Fx'))).toEqual(['endpoint']);
  });

  it('leads an endpoint opened from an error back to the error, closing the endpoint and its kind', () => {
    renderTrail('?error=fp-checkout&endpoint=POST+%2Forders&kind=http', 'endpoint');
    expect(screen.getAllByRole('button').map((button) => button.getAttribute('data-apm-drawer-crumb'))).toEqual(['error']);

    fireEvent.click(crumb('error'));

    const search = screen.getByTestId('search').textContent ?? '';
    expect(search).toContain('error=fp-checkout');
    expect(search).not.toContain('endpoint=');
    expect(search).not.toContain('kind=');
  });

  it('names an error and the endpoint opened from it under a trace, in that order', () => {
    renderTrail('?error=fp-checkout&endpoint=POST+%2Forders&kind=http', 'trace');
    expect(screen.getAllByRole('button').map((button) => button.getAttribute('data-apm-drawer-crumb'))).toEqual(['error', 'endpoint']);
  });
});
