import * as authApi from '@/api/auth';
import { useOrgStore } from '@/stores/useOrgStore';

import { useAuthStore } from './index';

/**
 * Signs the browser out: the server drops the session cookies, which scripts
 * cannot touch, this tab forgets the session and the other open tabs follow.
 */
export async function signOut(): Promise<void> {
  await authApi.signout().catch(() => undefined);
  useAuthStore.getState().logout({ allTabs: true });
  useOrgStore.getState().reset();
}
