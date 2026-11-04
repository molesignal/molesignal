import * as React from 'react';
import { useSearchParams } from 'react-router-dom';

import { EndpointDrawer } from './EndpointDrawer';
import type { ServiceAddress } from './useService';
import { ErrorDrawer } from '../errors/ErrorDrawer';
import { DRAWER_LAYERS, type DrawerLayer, drawerLayers } from '../frame/DrawerTrail';

/**
 * The endpoint and error drawers of a service page, stacked in the order
 * they were opened: the order their keys stand in the address. A drawer is
 * drawn afresh when its place in the stack changes, which puts it over the
 * drawers opened before it, where the one opened last always is; one that
 * closes keeps its place to slide away.
 */
export function ServiceDrawers({ address }: { address: ServiceAddress }) {
  const [searchParams] = useSearchParams();
  const layers = drawerLayers(searchParams);
  const [places, setPlaces] = React.useState<Record<DrawerLayer, string>>({ endpoint: 'endpoint', error: 'error' });
  const next = { ...places };
  for (const [index, layer] of layers.entries()) next[layer] = `${layer}@${index}`;
  const moved = DRAWER_LAYERS.some((layer) => next[layer] !== places[layer]);
  if (moved) setPlaces(next);

  const order = [...DRAWER_LAYERS.filter((layer) => !layers.includes(layer)), ...layers];

  return (
    <>
      {order.map((layer) =>
        layer === 'endpoint' ? <EndpointDrawer key={next.endpoint} address={address} /> : <ErrorDrawer key={next.error} />,
      )}
    </>
  );
}
