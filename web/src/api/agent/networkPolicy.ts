import { http } from '@/lib/http';

/**
 * The internal hosts and address ranges team members and individuals may
 * connect Mole Agent to (skill repositories, MCP servers). Everything else they
 * add can only reach the internet. Administrators read and change it.
 */
export interface MemberNetworkPolicy {
  private_hosts: string[];
  private_cidrs: string[];
  updated_by?: string | null;
  updated_at?: number | null;
}

export async function getNetworkPolicy(): Promise<MemberNetworkPolicy> {
  const { data } = await http.get<MemberNetworkPolicy>('/agent/network-policy');
  return data;
}

export async function putNetworkPolicy(input: {
  private_hosts: string[];
  private_cidrs: string[];
}): Promise<MemberNetworkPolicy> {
  const { data } = await http.put<MemberNetworkPolicy>('/agent/network-policy', input);
  return data;
}
