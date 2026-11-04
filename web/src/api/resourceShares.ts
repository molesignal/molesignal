import axios from 'axios';

import { http } from '@/lib/http';
import type { QueryResult } from '@/types/query';

export type ResourceShareMode =
  | 'authenticated'
  | 'cross_org'
  | 'public_link';
export type SharedResourceType = 'dashboard' | 'report';

export interface ResourceShare {
  id: string;
  organizationId: string;
  resourceType: SharedResourceType | 'report_file';
  resourceId: string;
  resourceVersionId: string | null;
  shareMode: ResourceShareMode;
  permissions: string[];
  constraints: Record<string, unknown>;
  expiresAt: number | null;
  maxViews: number | null;
  viewCount: number;
  allowDownload: boolean;
  enabled: boolean;
  crossOrgGrantId: string | null;
  snapshotContentType: string | null;
  snapshotFilename: string | null;
  createdBy: string;
  createdAt: number;
  lastAccessedAt: number | null;
  revokedAt: number | null;
  url: string | null;
}

export interface CreateResourceShareInput {
  resourceType: SharedResourceType;
  resourceId: string;
  shareMode: ResourceShareMode;
  resourceVersionId?: string;
  expiresInSecs?: number;
  password?: string;
  maxViews?: number;
  allowDownload?: boolean;
  constraints?: Record<string, unknown>;
  targetOrganizationId?: string;
  granteeType?: 'organization' | 'team' | 'user';
  granteeId?: string;
}

export interface CreateResourceShareResponse {
  share: ResourceShare;
  url: string;
}

export interface ResourceSharePolicy {
  organizationId: string;
  allowPublicLinks: boolean;
  allowPublicDashboards: boolean;
  maxPublicExpirySecs: number;
  requirePublicReportPassword: boolean;
  denyProductionPublicShares: boolean;
  allowPublicCsvDownload: boolean;
  updatedBy: string;
  updatedAt: number;
}

export interface PublicShareMetadata {
  kind: SharedResourceType;
  title?: string;
  format?: string;
  requiresPassword: boolean;
  allowDownload?: boolean;
  expiresAt?: number | null;
  generatedAt?: number;
  contentType?: string | null;
  constraints?: Record<string, unknown>;
  definition?: unknown;
  watermark?: {
    shareId: string;
    accessedAt: number;
  };
}

export interface PublicPanelQueryInput {
  panelId: string;
  refId: string;
  fromTime: number;
  toTime: number;
  /** The step of a PromQL panel's range query; a SQL panel ignores it. */
  step?: number;
  variables: Record<string, unknown>;
}

export async function create(
  input: CreateResourceShareInput,
): Promise<CreateResourceShareResponse> {
  const { data } = await http.post<CreateResourceShareResponse>(
    '/resource_shares',
    input,
  );
  return data;
}

export async function list(params?: {
  resourceType?: SharedResourceType;
  resourceId?: string;
}): Promise<ResourceShare[]> {
  const { data } = await http.get<ResourceShare[]>('/resource_shares', {
    params,
  });
  return data;
}

export async function revoke(id: string): Promise<ResourceShare> {
  const { data } = await http.delete<ResourceShare>(
    `/resource_shares/${encodeURIComponent(id)}`,
  );
  return data;
}

export async function rotate(
  id: string,
): Promise<CreateResourceShareResponse> {
  const { data } = await http.post<CreateResourceShareResponse>(
    `/resource_shares/${encodeURIComponent(id)}/rotate`,
  );
  return data;
}

export async function getPolicy(): Promise<ResourceSharePolicy> {
  const { data } = await http.get<ResourceSharePolicy>(
    '/resource_shares/policy',
  );
  return data;
}

export async function updatePolicy(
  policy: Omit<
    ResourceSharePolicy,
    'organizationId' | 'updatedBy' | 'updatedAt'
  >,
): Promise<ResourceSharePolicy> {
  const { data } = await http.put<ResourceSharePolicy>(
    '/resource_shares/policy',
    policy,
  );
  return data;
}

const publicHttp = axios.create({
  baseURL: '/api/v1/public/share',
  timeout: 30_000,
  withCredentials: true,
});

export async function publicMetadata(): Promise<PublicShareMetadata> {
  const { data } = await publicHttp.get<PublicShareMetadata>('');
  return data;
}

export async function unlock(password: string): Promise<void> {
  await publicHttp.post('/unlock', { password });
}

export async function runPublicPanelQuery(
  input: PublicPanelQueryInput,
): Promise<QueryResult> {
  const { data } = await publicHttp.post<QueryResult>('/query', input);
  return data;
}

export function publicFileUrl(download = false): string {
  return `/api/v1/public/share/file${download ? '?download=true' : ''}`;
}
