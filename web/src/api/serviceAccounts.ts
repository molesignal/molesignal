import * as usersApi from './users';

/**
 * Service-account list derived from the existing users endpoint. Accounts
 * with the `svc:` / `service:` naming convention are rendered as non-human
 * identities until a dedicated write API is available.
 */

export interface ServiceAccount {
  id: string;
  name: string;
  email: string;
}

export async function list(): Promise<ServiceAccount[]> {
  const users = await usersApi.list();
  return users
    .filter((u) => u.email.startsWith('svc:') || u.email.startsWith('service:'))
    .map((u) => ({ id: u.id, name: u.display_name || u.email, email: u.email }));
}
