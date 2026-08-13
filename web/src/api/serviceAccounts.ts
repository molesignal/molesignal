import { http } from '@/lib/http';

import type { CreatedApiToken } from './apiTokens';

export interface ServiceAccount {
  id: string;
  name: string;
  description: string;
  role_id: string;
  role_key: string;
  role_name: string;
  disabled: boolean;
  created_by: string;
  created_at_micros: number;
  updated_at_micros: number;
}

export interface CreateServiceAccountPayload {
  name: string;
  description?: string;
  role_id: string;
}

export interface InitialServiceAccountApiToken extends CreatedApiToken {
  name: string;
  service_account_id: string;
  expires_at_micros: number;
}

export interface ProvisionedServiceAccount {
  service_account: ServiceAccount;
  api_token: InitialServiceAccountApiToken;
}

export interface UpdateServiceAccountPayload {
  name?: string;
  description?: string;
  role_id?: string;
}

const accountPath = (id: string) =>
  `/service-accounts/${encodeURIComponent(id)}`;

export async function list(): Promise<ServiceAccount[]> {
  const { data } = await http.get<ServiceAccount[]>('/service-accounts');
  return data;
}

export async function create(
  payload: CreateServiceAccountPayload,
): Promise<ProvisionedServiceAccount> {
  const { data } = await http.post<ProvisionedServiceAccount>(
    '/service-accounts',
    payload,
  );
  return data;
}

export async function update(
  id: string,
  payload: UpdateServiceAccountPayload,
): Promise<ServiceAccount> {
  const { data } = await http.patch<ServiceAccount>(accountPath(id), payload);
  return data;
}

export async function setDisabled(
  id: string,
  disabled: boolean,
): Promise<ServiceAccount> {
  const { data } = await http.post<ServiceAccount>(
    `${accountPath(id)}/${disabled ? 'disable' : 'enable'}`,
  );
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(accountPath(id));
}
