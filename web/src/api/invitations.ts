import { http } from '@/lib/http';

export interface Invitation {
  id: string;
  orgId: string;
  email: string;
  roleId: string;
  roleKey: string;
  roleName: string;
  inviterId: string;
  status: 'pending' | 'accepted' | 'revoked' | string;
  sentAt: number;
  updatedAt: number;
}

export interface CreateInvitationInput {
  email: string;
  roleId?: string;
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
