import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useParams } from 'react-router-dom';

import { type ApmQueryParams, apmApi, apmQueryKeys, type ServiceIdentity } from '@/api/apm';

import { useApmScope } from '../frame/useApmScope';
import { apmQueryParams, compareEnvironment, type ServiceSection, serviceSectionOf } from '../scope';

/**
 * The service a service page is of, from its address: the name in the path,
 * the namespace and environment in the query. Together they are the service;
 * the version narrows it.
 */
export function useServiceAddress() {
  const params = useParams<{ service: string; section?: string }>();
  const apm = useApmScope();
  const name = params.service ?? '';
  const namespace = apm.searchParams.get('namespace') ?? '';
  const environment = apm.scope.environment;
  const section: ServiceSection | null = serviceSectionOf(params.section);
  const identity: ServiceIdentity = { namespace, name, environment };

  /** What the APM API is asked with for this service in the time in view (or the compared one). */
  const queryParams = React.useCallback(
    (range: { from: Date; to: Date }, extra: ApmQueryParams = {}): ApmQueryParams =>
      apmQueryParams(apm.scope, range, {
        service: name,
        ...(namespace ? { namespace } : {}),
        ...(apm.scope.version ? { version: apm.scope.version } : {}),
        ...extra,
      }),
    [apm.scope, name, namespace],
  );

  /** What the API is asked with for the compared stretch, in the compared environment when it is one; `null` while nothing is compared. */
  const compareParams = React.useCallback(
    (extra: ApmQueryParams = {}): ApmQueryParams | null => {
      if (!apm.compareRange) return null;
      const other = compareEnvironment(apm.scope.compare);
      return queryParams(apm.compareRange, { ...extra, ...(other ? { environment: other } : {}) });
    },
    [apm.compareRange, apm.scope.compare, queryParams],
  );

  return {
    ...apm,
    name,
    namespace,
    environment,
    section,
    identity,
    queryParams,
    compareParams,
  };
}

export type ServiceAddress = ReturnType<typeof useServiceAddress>;

/** The service's detail for the time in view, and for the compared stretch while a comparison is on. */
export function useServiceDetail(address: ServiceAddress) {
  const params = address.queryParams(address.range);
  const compareParams = address.compareParams();

  const detail = useQuery({
    queryKey: apmQueryKeys.service(address.orgId, address.name, params),
    queryFn: () => apmApi.service(address.name, params),
    enabled: Boolean(address.orgId && address.name),
    refetchInterval: 60_000,
  });

  const compared = useQuery({
    queryKey: apmQueryKeys.service(address.orgId, address.name, compareParams ?? {}),
    queryFn: () => apmApi.service(address.name, compareParams ?? {}),
    enabled: Boolean(address.orgId && address.name && compareParams),
  });

  return { detail, compared: compareParams ? compared : null };
}

/** The environments the service runs in, for the page's environment switch. */
export function useServiceEnvironments(address: ServiceAddress) {
  const params = apmQueryParams({ ...address.scope, environment: '' }, address.range, {
    service: address.name,
    ...(address.namespace ? { namespace: address.namespace } : {}),
    limit: 50,
  });

  const query = useQuery({
    queryKey: apmQueryKeys.services(address.orgId, params),
    queryFn: () => apmApi.services(params),
    enabled: Boolean(address.orgId && address.name),
    staleTime: 60_000,
  });

  return React.useMemo(() => [...new Set((query.data?.items ?? []).map((item) => item.service.environment))].sort(), [query.data]);
}
