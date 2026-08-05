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
  organization_id: string;
  resource_type: SharedResourceType | 'report_file';
  resource_id: string;
  resource_version_id: string | null;
  share_mode: ResourceShareMode;
  permissions: string[];
  constraints: Record<string, unknown>;
  expires_at: number | null;
  max_views: number | null;
  view_count: number;
  allow_download: boolean;
  enabled: boolean;
  cross_org_grant_id: string | null;
  snapshot_content_type: string | null;
  snapshot_filename: string | null;
  created_by: string;
  created_at: number;
  last_accessed_at: number | null;
  revoked_at: number | null;
  url: string | null;
}

export interface CreateResourceShareInput {
  resource_type: SharedResourceType;
  resource_id: string;
  share_mode: ResourceShareMode;
  resource_version_id?: string;
  expires_in_secs?: number;
  password?: string;
  max_views?: number;
  allow_download?: boolean;
  constraints?: Record<string, unknown>;
  target_organization_id?: string;
  grantee_type?: 'organization' | 'team' | 'user';
  grantee_id?: string;
}

export interface CreateResourceShareResponse {
  share: ResourceShare;
  url: string;
}

export interface ResourceSharePolicy {
  organization_id: string;
  allow_public_links: boolean;
  allow_public_dashboards: boolean;
  max_public_expiry_secs: number;
  require_public_report_password: boolean;
  deny_production_public_shares: boolean;
  allow_public_csv_download: boolean;
  updated_by: string;
  updated_at: number;
}

export interface PublicShareMetadata {
  kind: SharedResourceType;
  title?: string;
  format?: string;
  requires_password: boolean;
  allow_download?: boolean;
  expires_at_micros?: number | null;
  generated_at_micros?: number;
  content_type?: string | null;
  constraints?: Record<string, unknown>;
  definition?: unknown;
  watermark?: {
    share_id: string;
    accessed_at_micros: number;
  };
}

export interface PublicPanelQueryInput {
  panel_id: string;
  ref_id: string;
  from_micros: number;
  to_micros: number;
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
  resource_type?: SharedResourceType;
  resource_id?: string;
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
    'organization_id' | 'updated_by' | 'updated_at'
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
