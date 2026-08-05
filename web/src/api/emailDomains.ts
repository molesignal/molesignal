import { http } from '@/lib/http';

/** Org email-domain allowlist. Empty list = unrestricted (any domain may join). */
interface AllowlistResp {
  domains: string[];
}

export async function list(): Promise<string[]> {
  const { data } = await http.get<AllowlistResp>('/orgs/email-domains');
  return data.domains;
}

export async function add(domain: string): Promise<string[]> {
  const { data } = await http.post<AllowlistResp>('/orgs/email-domains', { domain });
  return data.domains;
}

export async function remove(domain: string): Promise<string[]> {
  const { data } = await http.delete<AllowlistResp>(
    `/orgs/email-domains/${encodeURIComponent(domain)}`,
  );
  return data.domains;
}
