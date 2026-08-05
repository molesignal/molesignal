import { create } from 'zustand';
import { type StateStorage, createJSONStorage, persist } from 'zustand/middleware';

export type Role = string;
export type AuthScope = 'organization' | 'system' | 'api_token';

export interface AssignedRole {
  id: string;
  key: string;
  name: string;
  builtin: boolean;
}

export interface AuthContext {
  user_id: string;
  org_id: string;
  display_role: string;
  roles: AssignedRole[];
  scope?: AuthScope;
  email?: string;
  display_name?: string;
  org_name?: string;
}

export type AuthContextInput = AuthContext;

interface AuthState {
  token: string | null;
  ctx: AuthContext | null;
  setSession: (token: string, ctx: AuthContextInput, remember?: boolean) => void;
  logout: () => void;
}

// 「记住我」标志：勾选时会话写 localStorage（持久），否则写 sessionStorage（关闭浏览器即清）。
const REMEMBER_KEY = 'molesignal-auth-remember';

/**
 * 记住我感知的持久化后端：`getItem` 先看 sessionStorage 再回落 localStorage；
 * `setItem` 按 REMEMBER_KEY 路由到对应存储并清掉另一处，避免两边各留一份过期会话。
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
      token: null,
      ctx: null,
      setSession: (token, ctx, remember) => {
        // signin 提交时带 remember；org 切换等内部调用不传 → 沿用上次选择。
        if (remember !== undefined) {
          localStorage.setItem(REMEMBER_KEY, remember ? 'true' : 'false');
        }
        const claims = decodeAuthTokenClaims(token);
        set({
          token,
          ctx: {
            ...ctx,
            display_role: normalizeRole(ctx.display_role),
            roles: ctx.roles,
            scope: normalizeAuthScope(claims?.scope ?? ctx.scope),
          },
        });
      },
      logout: () => set({ token: null, ctx: null }),
    }),
    { name: 'molesignal-auth', storage: createJSONStorage(() => rememberAwareStorage) },
  ),
);

export function normalizeRole(role: unknown): Role {
  return String(role ?? '').trim();
}

export function normalizeAuthScope(scope: unknown): AuthScope {
  if (scope === 'system') return 'system';
  if (scope === 'api_token') return 'api_token';
  return 'organization';
}

export interface AuthTokenClaims {
  org_id?: string;
  scope?: unknown;
}

/**
 * Reads access metadata from a JWT without treating the browser as an
 * authorization boundary. The backend still verifies the token and enforces
 * every permission; these claims only keep client routes/navigation aligned
 * with that server-side decision.
 */
export function decodeAuthTokenClaims(token: string | null): AuthTokenClaims | null {
  if (!token) return null;
  const encoded = token.split('.')[1];
  if (!encoded) return null;
  try {
    const normalized = encoded.replace(/-/g, '+').replace(/_/g, '/');
    const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=');
    return JSON.parse(atob(padded)) as AuthTokenClaims;
  } catch {
    return null;
  }
}
