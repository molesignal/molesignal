import * as React from 'react';

import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

import { serviceKey } from '../scope';

/** A service as favorites keep it: in every environment it runs in. */
export interface FavoriteService {
  namespace: string;
  name: string;
}

/** The most services the server keeps among one's favorites. */
export const MAX_FAVORITES = 200;

const NONE: readonly FavoriteService[] = [];

export function parseFavorites(raw: unknown): { services: FavoriteService[] } | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;
  const services = (raw as Record<string, unknown>).services;
  if (!Array.isArray(services)) return null;

  return {
    services: services.filter(
      (service): service is FavoriteService =>
        Boolean(service) && typeof service === 'object' && typeof service.namespace === 'string' && typeof service.name === 'string',
    ),
  };
}

/** The favorites with `service` added at the end, or taken out when it was one. */
export function toggledFavorites(services: readonly FavoriteService[], service: FavoriteService): FavoriteService[] {
  const key = serviceKey(service);
  if (services.some((kept) => serviceKey(kept) === key)) return services.filter((kept) => serviceKey(kept) !== key);

  return [...services, { namespace: service.namespace, name: service.name }].slice(-MAX_FAVORITES);
}

/**
 * The services the reader keeps at the top of the catalog, theirs alone and
 * kept as a preference, so another browser shows the same stars.
 */
export function useApmFavorites() {
  const kept = useUiPreference('apm.favorites', 'services', parseFavorites);
  const services = kept.value?.services ?? NONE;
  const keys = React.useMemo(() => new Set(services.map(serviceKey)), [services]);

  return {
    keys,
    isFavorite: (service: FavoriteService) => keys.has(serviceKey(service)),
    toggle: (service: FavoriteService) => kept.save({ services: toggledFavorites(services, service) }),
  };
}
