import { create } from 'zustand';
import { type StateStorage, createJSONStorage, persist } from 'zustand/middleware';

import { tabSessionSharing } from './tabSession';

export type Role = string;
export type AuthScope = 'organization' | 'system' | 'api_token';

export interface AssignedRole {
  id: string;
  key: string;
  name: string;
  builtin: boolean;
}

/**
 * Who the page shows as signed in. The session itself is in the browser's
 * HttpOnly cookies, which scripts cannot read; this is only what the server
 * answered when the session was opened, kept for display and routing.
 */
export interface AuthContext {
  userId: string;
  orgId: string;
  displayRole: string;
  roles: AssignedRole[];
  scope?: AuthScope;
  email?: string;
  displayName?: string;
  orgName?: string;
}

export type AuthContextInput = Omit<AuthContext, 'scope'> & { scope?: unknown };

interface AuthState {
  ctx: AuthContext | null;
  setSession: (ctx: AuthContextInput, remember?: boolean) => void;
  /** 结束本标签页的会话；`allTabs` 用于用户主动登出，同时让其它已打开的标签页一并登出。 */
  logout: (options?: { allTabs?: boolean }) => void;
}

// 「记住我」标志：勾选时身份写 localStorage（持久），否则写 sessionStorage（关闭浏览器即清），
// 与服务端写的 Cookie 一样：记住时持久，否则是会话 Cookie。
const REMEMBER_KEY = 'molesignal-auth-remember';

/**
 * 记住我感知的持久化后端：`getItem` 先看 sessionStorage 再回落 localStorage；
 * `setItem` 按 REMEMBER_KEY 路由到对应存储并清掉另一处，避免两边各留一份过期身份。
 */
const rememberAwareStorage: StateStorage = {
  getItem: (name) => sessionStorage.getItem(name) ?? localStorage.getItem(name),
  setItem: (name, value) => {
    if (localStorage.getItem(REMEMBER_KEY) === 'true') {
      localStorage.setItem(name, value);
      sessionStorage.removeItem(name);
    } else {
      sessionStorage.setItem(name, value);
      localStorage.removeItem(name);
    }
  },
  removeItem: (name) => {
    localStorage.removeItem(name);
    sessionStorage.removeItem(name);
  },
};

export const useAuthStore = create<AuthState>()(
  persist(
    (set) => ({
      ctx: null,
      setSession: (ctx, remember) => {
        // signin 提交时带 remember；org 切换等内部调用不传 → 沿用上次选择。
        if (remember !== undefined) {
          localStorage.setItem(REMEMBER_KEY, remember ? 'true' : 'false');
        }
        const next: AuthContext = {
          ...ctx,
          displayRole: normalizeRole(ctx.displayRole),
          roles: ctx.roles,
          scope: normalizeAuthScope(ctx.scope),
        };
        set({ ctx: next });
        tabSessionSharing.announceSession(sessionKey(next));
      },
      logout: (options) => {
        set({ ctx: null });
        if (options?.allTabs) tabSessionSharing.announceSignOut();
      },
    }),
    {
      name: 'molesignal-auth',
      storage: createJSONStorage(() => rememberAwareStorage),
      partialize: ({ ctx }) => ({ ctx }),
      // 1：身份改用 camelCase 的键名。之前存下的身份丢掉，标签页会再向服务端问一次会话。
      version: 1,
      migrate: () => ({ ctx: null }),
    },
  ),
);

/** Which user and organization a session is: `<userId>:<orgId>`. */
export function sessionKey(ctx: Pick<AuthContext, 'userId' | 'orgId'>): string {
  return `${ctx.userId}:${ctx.orgId}`;
}

export function normalizeRole(role: unknown): Role {
  return String(role ?? '').trim();
}

export function normalizeAuthScope(scope: unknown): AuthScope {
  if (scope === 'system') return 'system';
  if (scope === 'api_token') return 'api_token';
  return 'organization';
}
