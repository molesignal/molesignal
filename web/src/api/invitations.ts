import { http } from '@/lib/http';

export interface Invitation {
  id: string;
  org_id: string;
  email: string;
  role_id: string;
  role_key: string;
  role_name: string;
  inviter_id: string;
  status: 'pending' | 'accepted' | 'revoked' | string;
  sent_at_micros: number;
  updated_at_micros: number;
}

export interface CreateInvitationInput {
  email: string;
  role_id?: string;
}

export async function list(): Promise<Invitation[]> {
  const { data } = await http.get<Invitation[]>('/invitations');
  return data;
}

export async function create(input: CreateInvitationInput): Promise<Invitation> {
  const { data } = await http.post<Invitation>('/invitations', input);
  return data;
}

export async function resend(id: string): Promise<Invitation> {
  const { data } = await http.post<Invitation>(`/invitations/${encodeURIComponent(id)}/resend`);
  return data;
}

export async function revoke(id: string): Promise<Invitation> {
  const { data } = await http.post<Invitation>(`/invitations/${encodeURIComponent(id)}/revoke`);
  return data;
}
