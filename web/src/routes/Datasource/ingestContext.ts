import { useQuery } from '@tanstack/react-query';

import * as apiTokensApi from '@/api/apiTokens';
import * as instanceApi from '@/api/instance';
import { toApiError } from '@/lib/http';
import { useCurrentOrgSelection } from '@/stores/useOrgStore';

export interface IngestContext {
  endpoint: string;
  endpointHost: string;
  endpointPort: string;
  endpointScheme: string;
  endpointTls: string;
  token: string;
  tokenRole: string;
  tokenKind: apiTokensApi.ApiTokenKind | null;
  tokenExpiresAtMicros: number | null;
  tokenLoading: boolean;
  tokenError: string | null;
  orgId: string;
  orgLabel: string;
  applicationId: string;
  applicationValid: boolean;
  isRum: boolean;
}

export interface IngestContextOptions {
  isRum: boolean;
  applicationId: string;
}

export function isValidRumApplicationId(value: string): boolean {
  return /^[A-Za-z0-9._:-]{1,128}$/.test(value.trim());
}

function parseEndpoint(endpoint: string): { host: string; port: string; scheme: string } {
  try {
    const url = new URL(endpoint);
    const scheme = url.protocol.replace(/:$/, '') || 'https';
    return {
      host: url.hostname,
      port: url.port || (scheme === 'https' ? '443' : '80'),
      scheme,
    };
  } catch {
    return {
      host: endpoint.replace(/^https?:\/\//, ''),
      port: '443',
      scheme: 'https',
    };
  }
}

export function useIngestContext(options: IngestContextOptions): IngestContext {
  const { currentOrgId, orgLabel } = useCurrentOrgSelection();
  const applicationId = options.applicationId.trim();
  const applicationValid = !options.isRum || isValidRumApplicationId(applicationId);
  const instanceQuery = useQuery({
    queryKey: ['instance'],
    queryFn: instanceApi.get,
    staleTime: 300_000,
  });
  const tokenQuery = useQuery({
    queryKey: options.isRum
      ? ['rum-client-token', currentOrgId, applicationId]
      : ['default-ingestion-token', currentOrgId],
    queryFn: () =>
      options.isRum
        ? apiTokensApi.getRumClient(applicationId)
        : apiTokensApi.getDefault(),
    enabled: applicationValid && Boolean(currentOrgId),
    staleTime: options.isRum ? 15_000 : 300_000,
    gcTime: 0,
  });
  const endpoint = (instanceQuery.data?.external_url || window.location.origin).replace(/\/+$/, '');
  const { host, port, scheme } = parseEndpoint(endpoint);
  return {
    endpoint,
    endpointHost: host,
    endpointPort: port,
    endpointScheme: scheme,
    endpointTls: scheme === 'https' ? 'On' : 'Off',
    token: tokenQuery.data?.token ?? '',
    tokenRole: tokenQuery.data?.role_name ?? '—',
    tokenKind: tokenQuery.data?.token_kind ?? null,
    tokenExpiresAtMicros: tokenQuery.data?.expires_at_micros ?? null,
    tokenLoading: tokenQuery.isLoading || tokenQuery.isFetching,
    tokenError: tokenQuery.error ? toApiError(tokenQuery.error).message : null,
    orgId: currentOrgId ?? '',
    orgLabel,
    applicationId,
    applicationValid,
    isRum: options.isRum,
  };
}

export function substitute(content: string, context: IngestContext): string {
  return content
    .replaceAll('{{ENDPOINT_HOST}}', context.endpointHost)
    .replaceAll('{{ENDPOINT_PORT}}', context.endpointPort)
    .replaceAll('{{ENDPOINT_SCHEME}}', context.endpointScheme)
    .replaceAll('{{ENDPOINT_TLS}}', context.endpointTls)
    .replaceAll('{{ENDPOINT}}', context.endpoint)
    .replaceAll('{{APPLICATION_ID}}', context.applicationId || 'YOUR_APPLICATION_ID')
    .replaceAll('{{TOKEN}}', context.token || 'YOUR_INGESTION_TOKEN');
}
