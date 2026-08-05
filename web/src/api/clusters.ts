import { http } from '@/lib/http';

export interface RemoteCluster {
  id: string;
  name: string;
  advertise_addr: string;
  token_secret_ref: string;
  tls_verify: boolean;
  enabled: boolean;
  /** Discovered via gossip, not yet enabled/configured by an admin. */
  discovered: boolean;
  created_at_micros: number;
  updated_at_micros: number;
}

/** A node reported by a remote cluster's NodeService.List. */
export interface RemoteNode {
  node_id: string;
  advertise_addr: string;
  roles: string[];
  version: string;
}

export interface CreateClusterPayload {
  name: string;
  advertise_addr: string;
  /** Secret manager reference, never an inline token. */
  token_secret_ref: string;
  tls_verify?: boolean;
  enabled?: boolean;
}

export async function list(): Promise<RemoteCluster[]> {
  const { data } = await http.get<RemoteCluster[]>('/clusters');
  return data;
}

export async function create(payload: CreateClusterPayload): Promise<RemoteCluster> {
  const { data } = await http.post<RemoteCluster>('/clusters', {
    ...payload,
    tls_verify: payload.tls_verify ?? true,
    enabled: payload.enabled ?? true,
  });
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/clusters/${encodeURIComponent(id)}`);
}

/** Cross-cluster org mapping. Sender filters sendable orgs by these links;
 *  receiver resolves remote_org -> local_org (unmapped orgs are rejected). */
export interface OrgMap {
  remote_cluster_id: string;
  local_org_id: string;
  remote_org_id: string;
  /** Masked: `***` when a per-org token is set, empty otherwise. */
  token_secret_ref: string;
}

export interface OrgMapPayload {
  local_org_id: string;
  remote_org_id: string;
  /** Per-org token reference (`env:VAR` / `cipher_keys:<id>`); empty falls back to the cluster token. */
  token_secret_ref?: string;
}

export async function listOrgMap(clusterId: string): Promise<OrgMap[]> {
  const { data } = await http.get<OrgMap[]>(
    `/clusters/${encodeURIComponent(clusterId)}/org_map`,
  );
  return data;
}

export async function putOrgMap(clusterId: string, payload: OrgMapPayload): Promise<OrgMap> {
  const { data } = await http.put<OrgMap>(
    `/clusters/${encodeURIComponent(clusterId)}/org_map`,
    payload,
  );
  return data;
}

export async function deleteOrgMap(clusterId: string, localOrgId: string): Promise<void> {
  await http.delete(
    `/clusters/${encodeURIComponent(clusterId)}/org_map/${encodeURIComponent(localOrgId)}`,
  );
}

export async function listNodes(clusterId: string): Promise<RemoteNode[]> {
  const { data } = await http.get<RemoteNode[]>(
    `/clusters/${encodeURIComponent(clusterId)}/nodes`,
  );
  return data;
}
