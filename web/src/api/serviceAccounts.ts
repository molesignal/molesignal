import { http } from '@/lib/http';

import type { IssuedApiToken } from './apiTokens';

export interface ServiceAccount {
  id: string;
  name: string;
  description: string;
  roleId: string;
  roleKey: string;
  roleName: string;
  disabled: boolean;
  createdBy: string;
  createdAt: number;
  updatedAt: number;
}

export interface CreateServiceAccountPayload {
  name: string;
  description?: string;
  roleId: string;
}

export interface ProvisionedServiceAccount {
  serviceAccount: ServiceAccount;
  /** The account's first token, which may do whatever the account may. */
  apiToken: IssuedApiToken;
}

export interface UpdateServiceAccountPayload {
  name?: string;
  description?: string;
  roleId?: string;
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
