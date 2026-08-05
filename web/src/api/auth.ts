import { http } from '@/lib/http';
import type { AssignedRole } from '@/stores/auth';

export interface SigninResponse {
  token: string;
  user_id: string;
  email?: string;
  display_name?: string;
  org_id: string;
  org_name?: string;
  display_role: string;
  roles: AssignedRole[];
}

export async function signin(req: { email: string; password: string }): Promise<SigninResponse> {
  const { data } = await http.post<SigninResponse>('/auth/signin', req);
  return data;
}

export interface SignupResponse {
  /** "active"（已激活，附 token 直接登录）或 "pending"（待审批）。 */
  status: 'active' | 'pending' | string;
  token: string | null;
  user_id: string;
  email: string;
  display_name: string;
  org_id: string | null;
  org_name: string | null;
  display_role: string | null;
  roles: AssignedRole[];
}

export async function signup(req: {
  email: string;
  display_name: string;
  password: string;
}): Promise<SignupResponse> {
  const { data } = await http.post<SignupResponse>('/auth/signup', req);
  return data;
}

export async function forgotPassword(req: {
  email: string;
  locale?: string;
}): Promise<{ accepted: boolean }> {
  const { data } = await http.post<{ accepted: boolean }>('/auth/forgot-password', req);
  return data;
}

export async function resetPassword(req: {
  token: string;
  password: string;
}): Promise<{ reset: boolean }> {
  const { data } = await http.post<{ reset: boolean }>('/auth/reset-password', req);
  return data;
}
