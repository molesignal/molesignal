import { http } from '@/lib/http';
import type { PermissionKey } from '@/product/permissions';

/** One statement of a policy document: it allows the permissions its actions name. */
export interface PolicyStatement {
  Sid?: string;
  Effect: 'Allow';
  Action: string | string[];
}

/** A policy document, after the AWS IAM policy language. */
export interface PolicyDocument {
  Version: '1';
  Statement: PolicyStatement[];
}

/** What uses a policy in the organization: roles that attach it, working tokens that follow it. */
export interface PolicyUsage {
  roles: number;
  tokens: number;
  total: number;
}

/**
 * A named set of permissions. A built-in policy belongs to no organization and
 * cannot be changed; a custom one belongs to the organization.
 */
export interface IamPolicy {
  id: string;
  key: string;
  name: string;
  description: string;
  builtin: boolean;
  document: PolicyDocument;
  /** What the document allows, resolved when it was saved. */
  permissions: PermissionKey[];
  usage: PolicyUsage;
  createdBy?: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface CreatePolicyPayload {
  key: string;
  name: string;
  description?: string;
  document: unknown;
}

export interface UpdatePolicyPayload {
  name: string;
  description?: string;
  document: unknown;
}

export const POLICIES_QUERY_KEY = ['iam', 'policies'] as const;

export async function list(): Promise<IamPolicy[]> {
  const { data } = await http.get<IamPolicy[]>('/iam/policies');
  return data;
}

export async function create(payload: CreatePolicyPayload): Promise<IamPolicy> {
  const { data } = await http.post<IamPolicy>('/iam/policies', payload);
  return data;
}

export async function update(id: string, payload: UpdatePolicyPayload): Promise<IamPolicy> {
  const { data } = await http.patch<IamPolicy>(
    `/iam/policies/${encodeURIComponent(id)}`,
    payload,
  );
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/iam/policies/${encodeURIComponent(id)}`);
}
